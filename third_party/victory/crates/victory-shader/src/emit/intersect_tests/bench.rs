//! Synthetic frame timings of the translated node-test instruction.

use std::time::Instant;

use super::super::host::timed::{
    BufferUpdate, FrameTimes, MemoryPeak, TimedDispatch, process_peak_bytes,
};
use super::super::valu_tests::ONE_AT_A_TIME;
use super::*;

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;
const WARMUP: usize = 24;
const FRAMES: usize = 120;
const COHERENT_NODES: u32 = 128;
const CHILDREN: [u32; 4] = [0x108, 0x200, 0x308, 0x400];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workload {
    Static,
    Dynamic,
    Heavy,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Dynamic => "dynamic",
            Self::Heavy => "heavy",
        }
    }

    fn nodes(self, rays: u32) -> u32 {
        if self == Self::Heavy {
            rays
        } else {
            COHERENT_NODES
        }
    }

    fn case(self, index: u32, rays: u32) -> Case {
        let node = if self == Self::Heavy {
            // A permutation for power-of-two ray counts. Neighbouring lanes
            // fetch widely spaced nodes and alternate node types.
            index.wrapping_mul(104_729) % rays
        } else {
            (index / THREADS) % COHERENT_NODES
        };
        let ray_y = if index & 4 == 0 { 0.5 } else { 2.5 };
        let ray = Ray::new([0.0, node_y(node) + ray_y, 0.5], [1.0, 0.0, 0.0], 128.0);
        Some((node as usize, node_kind(node), ray))
    }
}

fn node_y(node: u32) -> f32 {
    ((node % COHERENT_NODES) / 8) as f32 * 4.0
}

fn node_kind(node: u32) -> u32 {
    match node % 4 {
        0 => 5, // 32-bit box
        1 => 4, // 16-bit box
        _ => 0, // triangle
    }
}

fn geometry(count: u32, offset_y: f32) -> Scene {
    let mut scene = Scene::new();
    let palette = count.min(COHERENT_NODES);
    for node in 0..palette {
        let x = 8.0 * ((node % 11) + 1) as f32;
        let y = node_y(node) + offset_y;
        let bounds = [12.0, 0.0, 8.0, 4.0].map(|dx| {
            let child_y = y + if dx == 8.0 { 3.0 } else { 0.0 };
            [x + dx, child_y, 0.0, x + dx + 1.0, child_y + 1.5, 1.0]
        });
        match node_kind(node) {
            5 => scene.add(nodes::box32(CHILDREN, bounds), 32),
            4 => scene.add(nodes::box16(CHILDREN, bounds), 16),
            _ => scene.add(
                nodes::triangles(&[[x, y, 0.0], [x, y + 2.0, 0.0], [x, y, 2.0]], 0),
                16,
            ),
        };
    }
    // Keep the exhaustive half-float fixture encoder out of the large loop.
    // The heavy scene still has a distinct buffer offset for every node.
    let palette_words = scene.words.len();
    let palette_starts = scene.starts.clone();
    for tile in 1..(count / palette) as usize {
        scene.words.extend_from_within(..palette_words);
        scene.starts.extend(
            palette_starts
                .iter()
                .map(|start| tile * palette_words + start),
        );
    }
    scene
}

fn shader() -> HostShader {
    let mut native = native(&ray_program(true, Descriptor::Table, true));
    let StageIo::Cs(cs) = &mut native.io else {
        unreachable!()
    };
    cs.rsrc2 |= 1 << 7; // COMPUTE_PGM_RSRC2.TGID_X_EN: group id x in s5.
    let (shader, resources) = build_compute_shader(&native).expect("the RT fixture lowers");
    assert_eq!(resources.bvh[0].1, BvhNodes::Resource);
    shader
}

struct Fixture {
    workload: Workload,
    rays: u32,
    initial: Scene,
    moved: Option<Scene>,
}

impl Fixture {
    fn new(workload: Workload, rays: u32) -> Self {
        assert!(rays.is_power_of_two() && rays >= THREADS);
        let nodes = workload.nodes(rays);
        Self {
            workload,
            rays,
            initial: geometry(nodes, 0.0),
            moved: (workload == Workload::Dynamic).then(|| geometry(nodes, 2.0)),
        }
    }

    fn input(&self) -> Vec<u32> {
        let record_words = (RECORD / 4) as usize;
        let node_offset = u64::from(self.rays) * u64::from(RECORD);
        let mut data = vec![0; node_offset as usize / 4];
        for (index, record) in data.chunks_exact_mut(record_words).enumerate() {
            let (node, kind, ray) = self.workload.case(index as u32, self.rays).unwrap();
            let pointer = (node_offset >> 3) + u64::from(self.initial.pointer(node, kind));
            ray_record(record, pointer, &ray);
        }
        data.extend_from_slice(&self.initial.words);
        data
    }

    fn validate(&self, phase: usize, rt: bool, output: &[u32]) {
        let scene = if phase == 0 {
            &self.initial
        } else {
            self.moved.as_ref().unwrap()
        };
        assert_eq!(output.len(), 4 * self.rays as usize);
        for (index, result) in output.chunks_exact(4).enumerate() {
            let case = self.workload.case(index as u32, self.rays);
            let want = if rt {
                expected(scene, &case, true)
            } else {
                BEFORE
            };
            assert_eq!(
                result,
                want,
                "{} phase {phase}, rt={rt}, ray {index}, case {case:x?}",
                self.workload.name()
            );
        }
    }
}

fn digest(words: &[u32]) -> blake3::Hash {
    // SAFETY: u32 has no padding. Windows targets are little endian.
    let bytes = unsafe { std::slice::from_raw_parts(words.as_ptr().cast(), size_of_val(words)) };
    blake3::hash(bytes)
}

fn rt_name(rt: bool) -> &'static str {
    if rt { "on" } else { "off" }
}

fn run(host: &Host, shader: &HostShader, fixture: &Fixture, warmup: usize, frames: usize) {
    let name = fixture.workload.name();
    let start_setup = Instant::now();
    let input = fixture.input();
    let mut dispatch = TimedDispatch::new(host, shader, &input, 4 * fixture.rays)
        .unwrap_or_else(|e| panic!("{name}: setup failed: {e}"));
    let uploads = fixture.moved.as_ref().map(|moved| {
        assert_eq!(fixture.initial.starts, moved.starts);
        assert_eq!(fixture.initial.words.len(), moved.words.len());
        [
            dispatch.stage(&fixture.initial.words).unwrap(),
            dispatch.stage(&moved.words).unwrap(),
        ]
    });
    let phases = if uploads.is_some() { 2 } else { 1 };
    let offset = u64::from(fixture.rays) * u64::from(RECORD);
    let bytes = size_of_val(fixture.initial.words.as_slice()) as u64;
    let setup_ms = start_setup.elapsed().as_secs_f64() * 1000.0;
    if frames > 0 {
        let moved = fixture.moved.as_ref().map(|s| digest(&s.words).to_string());
        println!(
            "input,{name},nodes={},node_bytes={bytes},initial_blake3={},moved_nodes_blake3={}",
            fixture.initial.starts.len(),
            digest(&input),
            moved.as_deref().unwrap_or("-")
        );
    }
    drop(input);
    let frame = |dispatch: &mut TimedDispatch, phase, rt, capture| {
        let update = uploads.as_ref().map(|sources| BufferUpdate {
            source: &sources[phase],
            offset,
            bytes,
        });
        dispatch
            .frame(fixture.rays / THREADS, rt, update, capture)
            .unwrap_or_else(|e| panic!("{name} phase {phase} rt={rt}: {e}"))
    };
    let start_warmup = Instant::now();
    for i in 0..warmup {
        let first = (i / phases) % 2 == 1;
        for rt in [first, !first] {
            frame(&mut dispatch, i % phases, rt, false);
        }
    }
    let warmup_ms = start_warmup.elapsed().as_secs_f64() * 1000.0;
    let mut samples = [Vec::with_capacity(frames), Vec::with_capacity(frames)];
    let mut peak = MemoryPeak::new(host);
    for i in 0..frames {
        // AB/BA: both modes see the same phase, with alternated pair order.
        let first = (i / phases) % 2 == 1;
        for rt in [first, !first] {
            samples[usize::from(rt)].push(frame(&mut dispatch, i % phases, rt, false));
            peak.sample();
        }
    }
    for phase in 0..phases {
        for rt in [false, true] {
            frame(&mut dispatch, phase, rt, true);
            let output = dispatch
                .captured()
                .expect("read back the verification result");
            fixture.validate(phase, rt, &output);
            if frames > 0 {
                println!(
                    "oracle,{name},{},{phase},blake3={}",
                    rt_name(rt),
                    digest(&output)
                );
            }
        }
    }
    if frames > 0 {
        println!("setup,{name},setup_ms={setup_ms:.6},warmup_ms={warmup_ms:.6}");
        for (rt, rows) in [false, true].into_iter().zip(&samples) {
            report(name, rt, phases, rows);
        }
        let (off, on) = (frame_stats(&samples[0]), frame_stats(&samples[1]));
        println!(
            "overhead,{name},frame_p50_ms={:.6},frame_p95_ms={:.6}",
            on.0 - off.0,
            on.1 - off.1
        );
        let bytes = |v: Option<u64>| v.map_or_else(|| "-".to_owned(), |n| n.to_string());
        println!(
            "memory,{name},sampled_local_peak_bytes={},sampled_nonlocal_peak_bytes={}",
            bytes(peak.local),
            bytes(peak.nonlocal)
        );
    }
}

fn stats(samples: impl Iterator<Item = f64>) -> (f64, f64) {
    let mut sorted = samples.collect::<Vec<_>>();
    sorted.sort_unstable_by(f64::total_cmp);
    let n = sorted.len();
    (
        (sorted[(n - 1) / 2] + sorted[n / 2]) / 2.0,
        sorted[(n * 95).div_ceil(100) - 1],
    )
}

fn frame_stats(rows: &[FrameTimes]) -> (f64, f64) {
    stats(rows.iter().map(|s| s.frame.as_secs_f64() * 1000.0))
}

fn report(name: &str, rt: bool, phases: usize, rows: &[FrameTimes]) {
    let rt = rt_name(rt);
    for (index, row) in rows.iter().enumerate() {
        let update = row
            .gpu_update_ms
            .map_or_else(|| "-".to_owned(), |ms| format!("{ms:.6}"));
        println!(
            "sample,{name},{rt},{},{},{:.6},{:.6},{update},{:.6},{:.6}",
            index + 1,
            index % phases,
            row.frame.as_secs_f64() * 1000.0,
            row.cpu_wait.as_secs_f64() * 1000.0,
            row.gpu_dispatch_ms,
            row.gpu_total_ms
        );
    }
    let phases = [
        ("frame", frame_stats(rows)),
        (
            "cpu_wait",
            stats(rows.iter().map(|r| r.cpu_wait.as_secs_f64() * 1000.0)),
        ),
        (
            "gpu_dispatch",
            stats(rows.iter().map(|r| r.gpu_dispatch_ms)),
        ),
        ("gpu_total", stats(rows.iter().map(|r| r.gpu_total_ms))),
    ];
    for (stage, (p50, p95)) in phases {
        println!("summary,{name},{rt},{stage},{p50:.6},{p95:.6}");
    }
    if rows[0].gpu_update_ms.is_some() {
        let (p50, p95) = stats(rows.iter().filter_map(|r| r.gpu_update_ms));
        println!("summary,{name},{rt},gpu_update,{p50:.6},{p95:.6}");
    }
}

#[test]
fn synthetic_rt_known_hits_and_percentiles() {
    let fixture = Fixture::new(Workload::Dynamic, 4 * THREADS);
    let hit = [CHILDREN[1], CHILDREN[3], CHILDREN[0], bvh::NONE];
    let miss = [bvh::NONE; 4];
    let moved = fixture.moved.as_ref().unwrap();
    let ray = Workload::Dynamic.case(0, fixture.rays);
    let other_ray = Workload::Dynamic.case(4, fixture.rays);
    assert_eq!(expected(&fixture.initial, &ray, true), hit);
    assert_eq!(expected(moved, &ray, true), miss);
    assert_eq!(expected(&fixture.initial, &other_ray, true), miss);
    assert_eq!(expected(moved, &other_ray, true), hit);
    assert_eq!(stats((1..=20).rev().map(f64::from)), (10.5, 19.0));
}

#[test]
fn synthetic_rt_writes_distinct_groups_and_uses_the_rt_switch() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let host = Host::new(false)
        .or_else(|_| Host::new(true))
        .expect("SM 6.6 adapter");
    let shader = shader();
    for workload in [Workload::Dynamic, Workload::Heavy] {
        run(&host, &shader, &Fixture::new(workload, 8 * THREADS), 0, 0);
    }
}

#[test]
#[ignore = "release benchmark; see docs/rt-benchmark.md"]
fn synthetic_rt_frames() {
    assert!(!cfg!(debug_assertions), "use the release profile");
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let host = Host::new(false).expect("the requested SM 6.6 adapter");
    let adapter = host.adapter_name().expect("DXGI adapter name");
    let start = Instant::now();
    let shader = shader();
    println!(
        "config,fixture=1,adapter={adapter},width={WIDTH},height={HEIGHT},rays={},samples_per_mode={FRAMES},warmup_per_mode={WARMUP},profile=release,translate_ms={:.6},dxil_blake3={}",
        WIDTH * HEIGHT,
        start.elapsed().as_secs_f64() * 1000.0,
        blake3::hash(&shader.dxil)
    );
    println!(
        "sample,scene,rt,frame_index,geometry_phase,frame_ms,cpu_wait_ms,gpu_update_ms,gpu_dispatch_ms,gpu_total_ms"
    );
    println!("summary,scene,rt,stage,p50_ms,p95_ms");
    for workload in [Workload::Static, Workload::Dynamic, Workload::Heavy] {
        run(
            &host,
            &shader,
            &Fixture::new(workload, WIDTH * HEIGHT),
            WARMUP,
            FRAMES,
        );
    }
    if let Some(peak) = process_peak_bytes() {
        println!("memory,process,peak_working_set_bytes={peak}");
    }
}
