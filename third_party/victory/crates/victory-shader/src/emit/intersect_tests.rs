//! The ray-tracing node test end to end: a synthetic compute program that
//! runs `image_bvh64_intersect_ray` (or the 32-bit form) once per thread,
//! lowered to DXIL, validated by `dxil.dll` when present, run on the host
//! GPU and WARP, and compared dword for dword with the CPU reference
//! ([`crate::bvh`]).
//!
//! The nodes are in a raw buffer. Two ways to name it are run: the
//! buffer's descriptor from the program's table (a resource of the host,
//! [`crate::resource::BvhNodes::Resource`]), and the title's way, a
//! descriptor of numbers over title memory with the structure's address in
//! the node pointer or in the descriptor. Some tests use a stand-in
//! (`lower::intersect::TEST_MEMORY`). The directory tests bind synthetic
//! copies of title memory and exercise the runtime's lookup and fault path.

use super::host::{Dispatch, Host, Root, Stride, View};
use super::lower::intersect::TEST_MEMORY;
use super::{HostShader, lower, validator};
use crate::bvh::{self, Ray, tests as nodes};
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::resource::BvhNodes;
use crate::{ShaderError, io, ir, resource, structure};

mod bench;

/// Threads of one group, each with its own record.
const THREADS: u32 = 32;
/// Bytes of a thread's record: the node pointer (two dwords), the ray's
/// extent, origin, direction and inverse direction, and whether the
/// thread runs the instruction.
const RECORD: u32 = 64;
/// Where the nodes start in the buffer.
const NODES: u32 = THREADS * RECORD;
/// What the result registers hold before the instruction.
const BEFORE: [u32; 4] = [1, 2, 3, 4];

/// How the program names the nodes' memory.
#[derive(Clone, Copy, PartialEq)]
enum Descriptor {
    /// The buffer's descriptor from the table.
    Table,
    /// Numbers, as the title builds it: dwords 0 and 1 (the base address
    /// in 256-byte units and the box sorting bit); the size and the rest of
    /// the flags as the title sets them.
    Numbers(u32, u32),
}

/// The descriptor the title builds for the 64-bit form: the whole address
/// space, sorted boxes, barycentrics returned.
const WHOLE: Descriptor = Descriptor::Numbers(0, bvh::BOX_SORT);

/// The program: per thread, the record's twelve operand dwords in v1-v12,
/// the instruction's result in v13-v16 (under a lane mask of the threads
/// whose record says so), stored to the UAV at 16 bytes a thread.
fn program(wide: bool, descriptor: Descriptor) -> Vec<u32> {
    ray_program(wide, descriptor, false)
}

fn ray_program(wide: bool, descriptor: Descriptor, global_rays: bool) -> Vec<u32> {
    let mut w = Vec::new();
    if global_rays {
        w.extend([
            0x8F15_8505, // s_lshl_b32 s21, s5, 5 (group id x * 32)
            0x4A00_0015, // v_add_nc_u32 v0, s21, v0 (global ray index)
        ]);
    }
    w.extend([
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202,
        0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0x0
        0xF408_0302,
        0xFA00_0020, // s_load_dwordx4 s[12:15], s[4:5], 0x20
        0x3428_0086, // v_lshlrev_b32 v20, 6, v0
        0xE038_1000,
        0x8002_0114, // buffer_load_dwordx4 v[1:4], v20, s[8:11], 0 offen
        0xE038_1010,
        0x8002_0514, // buffer_load_dwordx4 v[5:8], v20, s[8:11], 0 offen offset:16
        0xE038_1020,
        0x8002_0914, // buffer_load_dwordx4 v[9:12], v20, s[8:11], 0 offen offset:32
        0xE030_1030,
        0x8002_1114, // buffer_load_dword v17, v20, s[8:11], 0 offen offset:48
    ]);
    if global_rays {
        // The same PSO, input records and dispatch with only the RT switch changed.
        w.push(0x3622_2200); // v_and_b32 v17, s0, v17
    }
    for k in 0..4u32 {
        // v_mov_b32 v13+k, 1+k
        w.push(0x7E00_0200 | (13 + k) << 17 | (0x81 + k));
    }
    let rsrc = match descriptor {
        Descriptor::Table => 8,
        Descriptor::Numbers(word0, word1) => {
            // s_mov_b32 s24, word0
            w.extend([0xBE98_03FF, word0]);
            if word1 == bvh::BOX_SORT {
                // As the title makes it.
                w.push(0xBE99_0B81); // s_brev_b32 s25, 1
            } else {
                w.extend([0xBE99_03FF, word1]); // s_mov_b32 s25, word1
            }
            w.extend([
                0xBE9A_03C1, // s_mov_b32 s26, -1
                0xBE9B_03FF,
                0x8900_03FF, // s_mov_b32 s27, 0x890003ff
            ]);
            24
        }
    };
    w.extend([
        0xBE94_037E, // s_mov_b32 s20, exec_lo
        0x7DAA_2280, // v_cmpx_ne_u32 exec_lo, 0, v17
    ]);
    let first = 0x0000_0D01 | (rsrc >> 2) << 16;
    if wide {
        // image_bvh64_intersect_ray v[13:16], [v1 .. v12], s[rsrc:rsrc+3]
        w.extend([0xF19C_9F07, first, 0x0504_0302, 0x0908_0706, 0x000C_0B0A]);
    } else {
        // image_bvh_intersect_ray v[13:16], [v1, v3 .. v12], s[rsrc:rsrc+3]
        w.extend([0xF198_9F07, first, 0x0605_0403, 0x0A09_0807, 0x0000_0C0B]);
    }
    w.extend([
        0xBEFE_0314, // s_mov_b32 exec_lo, s20
        0x342A_0084, // v_lshlrev_b32 v21, 4, v0
        0xE078_1000,
        0x8003_0D15, // buffer_store_dwordx4 v[13:16], v21, s[12:15], 0 offen
        0xBF81_0000, // s_endpgm
    ]);
    w
}

fn native(words: &[u32]) -> NativeProgram {
    let table = |kind, at| UsedResource {
        kind,
        root: RootType::Table,
        table_offset_dwords: at,
        lower_bound: 0,
        stride: 4,
        dwords: 4,
        user_data: 4,
        slot: 1,
    };
    NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 22,
        layout: vec![
            RootParam {
                root: RootType::Constants,
                table_offset_dwords: 0,
                sgprs: 4,
                base_register: 0,
                user_data: 0,
                slot: 0,
            },
            RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 0,
                user_data: 4,
                slot: 1,
            },
        ],
        used: vec![table(ResourceType::Srv, 0), table(ResourceType::Uav, 8)],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 5 << 1,
            threads: [THREADS, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

/// Lowers the program; the DXIL is validated when the validator is there.
fn build(
    wide: bool,
    descriptor: Descriptor,
) -> Result<(HostShader, resource::Resources), ShaderError> {
    build_compute_shader(&native(&program(wide, descriptor)))
}

/// `native`, a compute program, translated (the tests' own way: no container).
pub(super) fn build_compute_shader(
    native: &NativeProgram,
) -> Result<(HostShader, resource::Resources), ShaderError> {
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st))?;
    let io = io::raise(&p, native).expect("io::raise");
    let lowered = lower::lower(&lower::Parts {
        native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
    })?;
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: lowered.dxil,
        user_data: lowered.user_data,
        samplers: lowered.samplers,
        static_samplers: Vec::new(),
        root_cbvs: lowered.root_cbvs,
        root_srvs: lowered.root_srvs,
        root_uavs: lowered.root_uavs,
        root_indexed_views: res.root_indexed_views.clone(),
    };
    if let Ok(mut v) = validator::Validator::load(validator::Validator::default_path().as_deref()) {
        let mut copy = shader.dxil.clone();
        v.validate(&mut copy).expect("DXIL validation failed");
    }
    Ok((shader, res))
}

/// One thread's test: a node (by its index in the node list), the kind its
/// pointer carries, and the ray; `None` leaves the thread masked out.
type Case = Option<(usize, u32, Ray)>;

/// The scene: node dwords (a 32-bit box node takes 32, the others 16) and
/// where each node starts, in dwords from the first node.
struct Scene {
    words: Vec<u32>,
    starts: Vec<usize>,
}

impl Scene {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            starts: Vec::new(),
        }
    }

    /// Adds a node of `dwords` (16 or 32) and returns its index.
    fn add(&mut self, node: [u32; 28], dwords: usize) -> usize {
        self.starts.push(self.words.len());
        let mut padded = [0u32; 32];
        padded[..28].copy_from_slice(&node);
        // Bytes the instruction must not read as part of the node: what
        // follows a 64-byte node, and a box node's last 16 bytes.
        for w in padded.iter_mut().skip(if dwords == 16 { 16 } else { 28 }) {
            *w = 0x7fc0_dead;
        }
        self.words.extend(&padded[..dwords]);
        self.starts.len() - 1
    }

    /// The pointer of node `n` with `kind`, counted from the first node.
    fn pointer(&self, n: usize, kind: u32) -> u32 {
        (4 * self.starts[n] as u32) >> 3 | kind
    }

    /// What the instruction reads of node `n`.
    fn node(&self, n: usize) -> [u32; 28] {
        std::array::from_fn(|k| self.words.get(self.starts[n] + k).copied().unwrap_or(0))
    }
}

/// A small generator of repeatable numbers.
struct Numbers(u64);

impl Numbers {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }

    /// A float in -1..1 in steps of 1/4096 (exactly representable).
    fn unit(&mut self) -> f32 {
        (self.next() % 8193) as f32 / 4096.0 - 1.0
    }

    fn vector(&mut self, scale: f32) -> [f32; 3] {
        [
            self.unit() * scale,
            self.unit() * scale,
            self.unit() * scale,
        ]
    }
}

/// The scene and batches of cases over it.
fn scene() -> (Scene, Vec<Vec<Case>>) {
    let mut s = Scene::new();
    let mut g = Numbers(0x5eed);
    const UNIT: [f32; 6] = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
    let moved =
        |b: [f32; 6], by: [f32; 3]| -> [f32; 6] { std::array::from_fn(|k| b[k] + by[k % 3]) };
    let off = [f32::NAN; 6];
    let mut box_nodes = Vec::new();
    // The boxes of the reference's own tests, in both widths.
    let lined = [
        moved(UNIT, [30.0, 0.0, 0.0]),
        moved(UNIT, [10.0, 0.0, 0.0]),
        moved(UNIT, [10.0, 5.0, 0.0]),
        moved(UNIT, [20.0, 0.0, 0.0]),
    ];
    let ids = [0x105, 0x20c, 0x304, 0x400];
    box_nodes.push((s.add(nodes::box32(ids, lined), 32), 5));
    box_nodes.push((s.add(nodes::box16(ids, lined), 16), 4));
    // Boxes around the origin, one switched off, one child missing.
    let around = [
        UNIT,
        moved(UNIT, [0.5, 0.5, 0.5]),
        off,
        [-4.0, -0.25, -0.25, 4.0, 0.25, 0.25],
    ];
    let ids2 = [8, bvh::NONE, 24, 0x1fff_fffc];
    box_nodes.push((s.add(nodes::box32(ids2, around), 32), 5));
    box_nodes.push((s.add(nodes::box16(ids2, around), 16), 4));
    // A child that is not there with inverted infinite bounds (the title's
    // builder leaves such slots): every ray "enters" it at minus infinity.
    let mut empty = lined;
    empty[1] = [f32::INFINITY, f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    let ids3 = [0x105, bvh::NONE, 0x304, 0x400];
    box_nodes.push((s.add(nodes::box32(ids3, empty), 32), 5));
    // Random boxes.
    for n in 0..6 {
        let boxes: [[f32; 6]; 4] = std::array::from_fn(|_| {
            let (c, h) = (g.vector(8.0), g.vector(2.0).map(f32::abs));
            std::array::from_fn(|k| c[k % 3] + if k < 3 { -h[k] } else { h[k - 3] })
        });
        let ids: [u32; 4] = std::array::from_fn(|k| 0x1000 * (n + 1) + 8 * k as u32 + 5);
        if n % 2 == 0 {
            box_nodes.push((s.add(nodes::box32(ids, boxes), 32), 5));
        } else {
            box_nodes.push((s.add(nodes::box16(ids, boxes), 16), 4));
        }
    }
    // Triangles: one of the reference's tests, a pair as the title's
    // builder stores it, all four triangles of a full node with turned
    // ids, random ones.
    let mut triangle_nodes = Vec::new();
    let front = [[0.0, 2.0, 5.0], [2.0, -2.0, 5.0], [-2.0, -2.0, 5.0]];
    triangle_nodes.push((s.add(nodes::triangles(&front, 9), 16), 0));
    let pair = s.add(
        nodes::triangles(
            &[front[0], front[1], front[2], [2.0, 2.0, 5.0]],
            9 | 0x80 | 6 << 8,
        ),
        16,
    );
    triangle_nodes.extend([(pair, 0), (pair, 1)]);
    let five = [
        [-3.0, -1.0, 6.0],
        [-1.0, 2.0, 6.5],
        [1.0, -1.0, 6.0],
        [3.0, 2.0, 6.5],
        [5.0, -1.0, 7.0],
    ];
    // I and J per triangle: (1, 2), (2, 0), (0, 1), (1, 2).
    let full = s.add(nodes::triangles(&five, 9 | 2 << 8 | 4 << 16 | 9 << 24), 16);
    triangle_nodes.extend((0..4).map(|k| (full, k)));
    for _ in 0..5 {
        let c = g.vector(6.0);
        let v: [[f32; 3]; 3] = std::array::from_fn(|_| {
            let d = g.vector(3.0);
            [c[0] + d[0], c[1] + d[1], c[2] + d[2]]
        });
        triangle_nodes.push((s.add(nodes::triangles(&v, 9), 16), 0));
    }
    let mut cases: Vec<Case> = Vec::new();
    // Rays of the reference's tests against the lined-up boxes.
    for &(n, kind) in &box_nodes[..2] {
        for (origin, direction, extent) in [
            ([0.0; 3], [1.0, 0.0, 0.0], 1000.0),
            ([0.0; 3], [1.0, 0.0, 0.0], 19.5),
            ([0.0; 3], [1.0, 0.0, 0.0], 19.0),
            ([0.0; 3], [-1.0, 0.0, 0.0], 1000.0),
            ([40.0, 0.0, 0.0], [-2.0, 0.0, 0.0], 1000.0),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], 1000.0),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 1000.0),
            ([0.0, 0.5, 0.25], [1.0, -0.0, 0.0], 1000.0),
            ([0.0; 3], [0.0; 3], 1000.0),
        ] {
            cases.push(Some((n, kind, Ray::new(origin, direction, extent))));
        }
    }
    // Random rays at every box node, half of them aimed at a box.
    for &(n, kind) in &box_nodes {
        let words = s.node(n);
        let boxes = if kind == 5 {
            bvh::boxes32(&words)
        } else {
            bvh::boxes16(&words)
        };
        for k in 0..12 {
            let origin = g.vector(12.0);
            let direction = if k % 2 == 0 {
                let b = boxes[(k / 2) % 4];
                if b[0].is_nan() {
                    g.vector(1.0)
                } else {
                    std::array::from_fn(|a| (b[a] + b[3 + a]) / 2.0 - origin[a] + g.unit() / 2.0)
                }
            } else {
                g.vector(1.0)
            };
            let extent = if k % 3 == 0 { 6.0 } else { 100.0 };
            cases.push(Some((n, kind, Ray::new(origin, direction, extent))));
        }
    }
    // Rays at every triangle: aimed inside, at an edge, at a vertex, past
    // it, from both sides, along each axis.
    for &(n, kind) in &triangle_nodes {
        let p = bvh::triangle_vertices(&s.node(n), kind);
        let at = |w: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|a| w[0] * p[0][a] + w[1] * p[1][a] + w[2] * p[2][a])
        };
        for (k, w) in [
            [0.5, 0.25, 0.25],
            [0.125, 0.75, 0.125],
            [0.5, 0.5, 0.0],
            [0.0, 0.5, 0.5],
            [1.0, 0.0, 0.0],
            [0.75, 0.75, -0.5],
            [-0.25, 0.5, 0.75],
            [0.25, 0.25, 0.5],
        ]
        .into_iter()
        .enumerate()
        {
            let target = at(w);
            let origin = if k % 2 == 0 { [0.0; 3] } else { g.vector(16.0) };
            let mut direction: [f32; 3] = std::array::from_fn(|a| target[a] - origin[a]);
            if k == 7 {
                // The triangle lies behind.
                direction = direction.map(|d| -d);
            }
            cases.push(Some((n, kind, Ray::new(origin, direction, 100.0))));
        }
        for axis in 0..3 {
            for sign in [1.0f32, -1.0] {
                let target = at([0.25, 0.5, 0.25]);
                let mut origin = target;
                origin[axis] -= 8.0 * sign;
                let mut direction = [0.0; 3];
                direction[axis] = sign;
                cases.push(Some((n, kind, Ray::new(origin, direction, 100.0))));
            }
        }
    }
    // Kinds the instruction does not read, and threads masked out.
    let ray = Ray::new([0.0; 3], [0.0, 0.0, 1.0], 100.0);
    cases.extend([
        Some((triangle_nodes[0].0, 6, ray)),
        Some((triangle_nodes[0].0, 7, ray)),
        None,
        Some((
            box_nodes[0].0,
            5,
            Ray::new([0.0; 3], [1.0, 0.0, 0.0], 1000.0),
        )),
        None,
    ]);
    let batches = cases
        .chunks(THREADS as usize)
        .map(<[Case]>::to_vec)
        .collect();
    (s, batches)
}

/// The buffer for one batch: the threads' records, then the nodes. Node
/// pointers count from `base` before the first node, in bytes.
fn buffer(scene: &Scene, batch: &[Case], base: u64) -> Vec<u32> {
    let mut data = vec![0u32; (NODES / 4) as usize];
    for (t, case) in batch.iter().enumerate() {
        let Some((n, kind, ray)) = case else { continue };
        let pointer = (base >> 3) + u64::from(scene.pointer(*n, *kind));
        ray_record(&mut data[t * (RECORD / 4) as usize..], pointer, ray);
    }
    data.extend(&scene.words);
    data
}

fn ray_record(r: &mut [u32], pointer: u64, ray: &Ray) {
    (r[0], r[1]) = (pointer as u32, (pointer >> 32) as u32);
    r[2] = ray.extent.to_bits();
    for a in 0..3 {
        r[3 + a] = ray.origin[a].to_bits();
        r[6 + a] = ray.direction[a].to_bits();
        r[9 + a] = ray.inverse[a].to_bits();
    }
    r[12] = 1;
}

fn expected(scene: &Scene, case: &Case, sort: bool) -> [u32; 4] {
    match case {
        Some((n, kind, ray)) => bvh::intersect(&scene.node(*n), *kind, ray, sort),
        None => BEFORE,
    }
}

/// Sets the tests' title memory for this thread until dropped.
struct Memory;

impl Memory {
    fn at(base: u64) -> Self {
        // Heap slot 0: the buffer of records and nodes.
        TEST_MEMORY.set(Some((0, base)));
        Self
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        TEST_MEMORY.set(None);
    }
}

/// Runs every batch on the hardware adapter and WARP and compares with
/// the reference. `pointers` is what node pointers count from, in bytes
/// before the first node.
fn run(wide: bool, descriptor: Descriptor, pointers: u64, sort: bool) {
    let (shader, res) = build(wide, descriptor).expect("the program lowers");
    assert_eq!(res.bvh.len(), 1);
    let (scene, batches) = scene();
    // The reference must see a bit of everything, or the comparison
    // proves little.
    let all: Vec<&Case> = batches.iter().flatten().collect();
    let results: Vec<[u32; 4]> = all.iter().map(|c| expected(&scene, c, sort)).collect();
    let boxes = |hits: usize| {
        all.iter()
            .zip(&results)
            .filter(|(c, r)| {
                matches!(c, Some((_, 4 | 5, _)))
                    && r.iter().filter(|&&x| x != bvh::NONE).count() == hits
            })
            .count()
    };
    let triangles = |hit: bool| {
        all.iter()
            .zip(&results)
            .filter(|(c, r)| {
                matches!(c, Some((_, 0..=3, _))) && (r[0] != f32::INFINITY.to_bits()) == hit
            })
            .count()
    };
    assert!(
        boxes(0) > 5 && boxes(1) > 5 && boxes(2) > 5 && boxes(3) > 0,
        "box cases"
    );
    assert!(
        triangles(true) > 30 && triangles(false) > 30,
        "triangle cases"
    );
    // Sorting must matter to some case, or the unsorted run proves nothing.
    assert!(
        all.iter()
            .any(|c| expected(&scene, c, true) != expected(&scene, c, false)),
        "no case tells sorted from unsorted"
    );
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        for (b, batch) in batches.iter().enumerate() {
            let data = buffer(&scene, batch, pointers);
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants(vec![0; 5]))],
                        views: &[
                            View::Buffer {
                                data: &data,
                                stride: Stride::Raw,
                            },
                            View::RawUav(4 * THREADS),
                        ],
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            for t in 0..THREADS as usize {
                let case = batch.get(t).copied().flatten();
                let want = match batch.get(t) {
                    Some(c) => expected(&scene, c, sort),
                    // A thread past the batch: its record is zeros, so it
                    // is masked out.
                    None => BEFORE,
                };
                let got = &out[0][4 * t..4 * t + 4];
                assert_eq!(
                    got,
                    want,
                    "{label}, batch {b} thread {t}: {case:x?}\n got {:x?} = {:?}\nwant {:x?} = {:?}",
                    got,
                    got.iter().map(|&x| f32::from_bits(x)).collect::<Vec<_>>(),
                    want,
                    want.map(f32::from_bits),
                );
            }
        }
        ran += 1;
    }
    assert!(ran > 0, "no SM 6.6 device available");
}

#[test]
fn the_64_bit_node_test_in_a_host_buffer_matches_the_reference_on_the_card() {
    // Pointers count from the buffer's start.
    run(true, Descriptor::Table, u64::from(NODES), true);
    assert_eq!(
        build(true, Descriptor::Table).unwrap().1.bvh[0].1,
        BvhNodes::Resource
    );
}

#[test]
fn the_32_bit_node_test_in_a_host_buffer_matches_the_reference_on_the_card() {
    run(false, Descriptor::Table, u64::from(NODES), true);
}

/// Where the tests' title memory puts the buffer: past 4 GiB, on 256 bytes.
const BASE: u64 = 0x12_3456_7800;

#[test]
fn the_64_bit_node_test_over_title_memory_matches_the_reference_on_the_card() {
    // The title's way: one descriptor for all of memory, the structure's
    // address folded into a 64-bit pointer.
    let _memory = Memory::at(BASE);
    run(true, WHOLE, BASE + u64::from(NODES), true);
}

#[test]
fn the_32_bit_node_test_over_title_memory_matches_the_reference_on_the_card() {
    // The title's other way: the structure's address in the descriptor
    // (in 256-byte units, 40 bits), a 32-bit pointer from there. Here with
    // an address past 2^40 bytes, and without box sorting.
    let base = 0x312_3456_7800u64;
    let structure = base + u64::from(NODES);
    let _memory = Memory::at(base);
    let descriptor = Descriptor::Numbers((structure >> 8) as u32, (structure >> 40) as u32);
    run(false, descriptor, 0, false);
    // And sorted.
    let descriptor = Descriptor::Numbers(
        (structure >> 8) as u32,
        (structure >> 40) as u32 | bvh::BOX_SORT,
    );
    run(false, descriptor, 0, true);
}

#[test]
fn a_descriptor_of_numbers_is_title_memory_read_through_the_directory() {
    for wide in [true, false] {
        let native = native(&program(wide, WHOLE));
        let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
        let st = structure::structure(&p).expect("structure");
        let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("tracked");
        // The flags the title sets: sorted boxes, barycentrics, a
        // ray-tracing structure. No resource is taken for it: the buffer
        // loads' and the store's descriptors are all there is.
        assert_eq!(res.bvh.len(), 1);
        assert_eq!(
            res.bvh[0].1,
            BvhNodes::Memory {
                word1: 0x8000_0000,
                word3: 0x8900_03ff
            }
        );
        assert_eq!(res.descriptors.len(), 5);
        // Without the tests' stand-in the nodes are read as the title's
        // programs will read them: through the host's directory of title
        // memory. The program lowers and validates, and asks the host for
        // the directory and for the list of pages it found no copy of.
        let (shader, _) = build(wide, WHOLE).expect("lowers through the directory");
        assert!(shader.root_srvs.contains(&super::SLOT_MEMORY));
        assert!(shader.root_uavs.contains(&super::SLOT_FAULTS));
    }
}

#[test]
fn a_structure_that_returns_triangle_ids_is_refused_by_name() {
    // Triangle return mode 0 (bit 24 of dword 3 clear): the last two
    // result dwords would be a triangle id and a hit status.
    let mut words = program(true, WHOLE);
    let flags = words.iter().position(|&w| w == 0x8900_03FF).unwrap();
    words[flags] = 0x8800_03FF;
    let native = native(&words);
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("tracked");
    let io = io::raise(&p, &native).expect("io::raise");
    let r = lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
    });
    match r {
        Err(ShaderError::Unsupported { what, .. }) => {
            assert_eq!(what, "a ray-tracing structure that returns triangle ids");
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

/// A buffer view picked from the table at an index the program computes,
/// whose dwords 0 and 1 the program reads as numbers (the tracer takes its
/// structure's address so, by an index from a constant buffer): they are
/// the view's title address from the host's table by view heap slot, after
/// the strides in the same root view.
#[test]
fn a_picked_views_address_comes_from_the_hosts_table() {
    use super::{SLOT_STRIDES, SLOT_STRIDES_BYTES, VIEW_ADDRESSES};
    let words = [
        0xBE85_0380, // s_mov_b32 s5, 0
        0x8F06_8500, // s_lshl_b32 s6, s0, 5
        0xF408_0202,
        0x0C00_0000, // s_load_dwordx4 s[8:11], s[4:5], s6
        0xF408_0302,
        0xFA00_0020, // s_load_dwordx4 s[12:15], s[4:5], 0x20
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        0x7E02_0208, // v_mov_b32 v1, s8
        0x7E04_0209, // v_mov_b32 v2, s9
        0x342A_0083, // v_lshlrev_b32 v21, 3, v0
        0xE074_1000,
        0x8003_0115, // buffer_store_dwordx2 v[1:2], v21, s[12:15], 0 offen
        0xBF81_0000, // s_endpgm
    ];
    let (shader, _) = build_compute_shader(&native(&words)).expect("the program lowers");
    assert!(shader.root_srvs.contains(&SLOT_STRIDES), "{:?}", shader.root_srvs);
    assert!(
        !shader.user_data.iter().any(|u| u.root == RootType::Other(super::USER_VIEW_ADDRESS)),
        "no host word for a fixed place"
    );
    let mut table = vec![0u32; SLOT_STRIDES_BYTES as usize / 4];
    let at = VIEW_ADDRESSES as usize / 4;
    // Slot 2, the one picked; slot 0 as a decoy.
    table[at..at + 2].copy_from_slice(&[0xdead_0000, 0xdd]);
    table[at + 4..at + 6].copy_from_slice(&[0x1234_5600, 0x42]);
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let decoy = [7u32; 4];
        let out = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    // s0: the picked view's place in the table (slot 2).
                    roots: vec![
                        (0, Root::Constants(vec![2, 0, 0, 0, 0])),
                        (1, Root::View { srv: true, data: table.clone() }),
                    ],
                    views: &[
                        View::Buffer { data: &decoy, stride: Stride::Raw },
                        View::RawUav(8 * THREADS),
                        View::Buffer { data: &decoy, stride: Stride::Raw },
                    ],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        for t in 0..THREADS as usize {
            assert_eq!(out[0][2 * t..2 * t + 2], [0x1234_5600, 0x42], "{label} thread {t}");
        }
        ran += 1;
    }
    assert!(ran > 0, "no SM 6.6 device available");
}

/// A register that holds a lane mask on one path and a number on the other
/// (the ray tracer's SGPRs reused between saved masks and constant-buffer
/// fields), joined where the wave takes one edge, then cut with mask
/// operations and read as a number: the mask operations compute on the
/// numbers (`sop`), and the read is the console's register.
#[test]
fn mask_operations_on_a_joined_number_give_the_number() {
    let words = [
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0302,
        0xFA00_0020, // s_load_dwordx4 s[12:15], s[4:5], 0x20
        0x7D88_0098, // v_cmp_gt_u32 vcc_lo, 24, v0
        0x8706_7E6A, // s_and_b32 s6, vcc_lo, exec_lo (lanes 0-23)
        0xBF06_8000, // s_cmp_eq_u32 s0, 0
        0xBF85_0001, // s_cbranch_scc1 +1
        0x8106_8101, // s_add_i32 s6, s1, 1 (a number)
        0x8707_FF06,
        0x00FF_00FF, // s_and_b32 s7, s6, 0x00ff00ff
        0x8808_0207, // s_or_b32 s8, s7, s2
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        0x7E02_0208, // v_mov_b32 v1, s8
        0x342A_0082, // v_lshlrev_b32 v21, 2, v0
        0xE070_1000,
        0x8003_0115, // buffer_store_dword v1, v21, s[12:15], 0 offen
        0xBF81_0000, // s_endpgm
    ];
    // On the first try refused, as before (programs that translate so do
    // not change); `translate` tries such a program once more with mask
    // operations on numbers read as numbers.
    match build_compute_shader(&native(&words)) {
        Err(ShaderError::Unsupported { what, .. }) => assert_eq!(what, lower::MASK_ELSEWHERE),
        other => panic!("{:?}", other.map(|_| ())),
    }
    let (shader, _) = lower::with_mask_ops_on_numbers(|| build_compute_shader(&native(&words)))
        .expect("the program lowers");
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // The hardware device only: the program declares the console's 32
    // lanes (its wave operations), which WARP does not take.
    let mut ran = 0;
    for (label, warp) in [("hardware", false)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = match host.compute_pipeline(&shader) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("the {label} device does not take this program (waves of 64?): {e}; not run");
                continue;
            }
        };
        let unused = [0u32; 4];
        for (constants, want) in [
            // The mask's path: lanes 0-23 as the number.
            ([0, 0x1234_5678, 0x0100_0000], 0x01FF_00FF),
            // The number's path.
            ([1, 0x1234_5678, 0x0100_0000], 0x0134_0079),
        ] {
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants([&constants[..], &[0, 0]].concat()))],
                        views: &[View::Buffer { data: &unused, stride: Stride::Raw }, View::RawUav(4 * THREADS)],
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            for t in 0..THREADS as usize {
                assert_eq!(out[0][t], want, "{label} {constants:x?} thread {t}");
            }
        }
        ran += 1;
    }
    if ran == 0 {
        eprintln!("not run on a device; lowered only");
    }
}

/// Builds the user-data root constant buffer for a shader that reads
/// `USER_MEMORY_BASE` (and optionally `USER_DISPATCH_THREADS_X`) in addition
/// to the standard root constants `s0..s3` and table offset `s4`.
fn user_data_constants(
    shader: &HostShader,
    s0_to_s3: [u32; 4],
    table_bytes: u32,
    memory_base: u32,
    dispatch_threads_x: u32,
) -> Vec<u32> {
    let mut ud = vec![0u32; shader.user_data_dwords().max(5) as usize];
    for u in &shader.user_data {
        let val = match u.root {
            RootType::Constants => s0_to_s3.get(u.dword as usize).copied().unwrap_or(0),
            RootType::Table => table_bytes,
            RootType::Other(k) if k == super::USER_MEMORY_BASE => memory_base,
            RootType::Other(k) if k == super::USER_DISPATCH_THREADS_X => dispatch_threads_x,
            _ => 0,
        };
        ud[u.index as usize] = val;
    }
    ud
}

/// Builds a compact `SLOT_MEMORY` directory buffer (covering pages `0..pages`,
/// with the last entry kept empty as the clamp sink).
/// Each mapping is `(page, copy_number_1based, byte_offset, read_only)`.
fn make_directory(pages: u32, mappings: &[(u32, u32, u32, bool)]) -> Vec<u32> {
    let mut dir = vec![0u32; pages as usize * 2];
    for &(page, copy, offset, ro) in mappings {
        let at = page as usize * 2;
        dir[at] = copy | if ro { super::MEMORY_READ_ONLY } else { 0 };
        dir[at + 1] = offset;
    }
    dir
}

/// Returns all non-zero faulted page numbers recorded in `SLOT_FAULTS`.
fn faulted_pages(faults: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = faults.iter().skip(1).copied().filter(|&p| p != 0).collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn directory_records(entries: &[(bool, u64, Ray)]) -> Vec<u32> {
    let mut data = vec![0u32; (THREADS * RECORD / 4) as usize];
    for (t, &(active, pointer, ray)) in entries.iter().enumerate() {
        let r = &mut data[t * (RECORD / 4) as usize..][..13];
        r[0] = pointer as u32;
        r[1] = (pointer >> 32) as u32;
        r[2] = ray.extent.to_bits();
        for a in 0..3 {
            r[3 + a] = ray.origin[a].to_bits();
            r[6 + a] = ray.direction[a].to_bits();
            r[9 + a] = ray.inverse[a].to_bits();
        }
        r[12] = u32::from(active);
    }
    data
}

#[test]
fn bvh_directory_address_carries_past_four_gib() {
    const PAGE_BYTES: usize = 1 << 16;
    const WORD0: u32 = 0x00ff_ffff;
    let bounds = [30.0, 10.0, 40.0, 20.0].map(|x| [x, -1.0, -1.0, x + 1.0, 1.0, 1.0]);
    let triangle = nodes::triangles(&[[5.0, 0.0, 2.0], [5.0, 2.0, -2.0], [5.0, -2.0, -2.0]], 9);
    let triangle_hit = [80.0, 16.0, 4.0, 4.0].map(f32::to_bits);
    // The two box nodes give different child IDs if the high pointer word is lost.
    let placements = [
        (0x00, 0xffff_ff00, triangle, triangle_hit),
        (
            0x25,
            0x1_0000_0000,
            nodes::box32([0x105, 0x20c, 0x304, 0x400], bounds),
            [0x20c, 0x400, 0x105, 0x304],
        ),
        (0x2020, 0x1_0001_0000, triangle, triangle_hit),
        (
            0x1_0000_0025,
            0x9_0000_0000,
            nodes::box32([0x505, 0x60c, 0x704, 0x800], bounds),
            [0x60c, 0x800, 0x505, 0x704],
        ),
    ];
    let mut arena = vec![0; placements.len() * PAGE_BYTES / 4];
    let mut mappings = Vec::with_capacity(placements.len());
    let mut entries = Vec::new();
    let mut expected = Vec::new();
    let hit = Ray::new([0.0; 3], [1.0, 0.0, 0.0], 100.0);
    let miss = Ray::new([0.0, 3.0, 0.0], [1.0, 0.0, 0.0], 100.0);
    for (i, (pointer, address, node, hit_words)) in placements.into_iter().enumerate() {
        assert_eq!(bvh::node_address(WORD0, bvh::BOX_SORT, pointer), address);
        let page_start = i * PAGE_BYTES;
        let byte = page_start + (address % PAGE_BYTES as u64) as usize;
        arena[byte / 4..][..node.len()].copy_from_slice(&node);
        mappings.push((
            (address / PAGE_BYTES as u64) as u32,
            1,
            page_start as u32,
            false,
        ));
        let kind = bvh::kind(pointer);
        let miss_words = if kind == 5 {
            [bvh::NONE; 4]
        } else {
            bvh::MISS.map(f32::to_bits)
        };
        for (ray, words) in [(hit, hit_words), (miss, miss_words)] {
            entries.push((true, pointer, ray));
            let cpu = bvh::intersect(&node, kind, &ray, true);
            assert_eq!(cpu, words, "CPU result for {pointer:#x}, ray {ray:?}");
            expected.push(cpu);
        }
    }
    let directory = make_directory(mappings.last().unwrap().0 + 2, &mappings);
    let descriptor = Descriptor::Numbers(WORD0, bvh::BOX_SORT);

    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for warp in [false, true] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 device for warp={warp}; skipped");
            continue;
        };
        let adapter = host.adapter_name();
        eprintln!("BVH 4 GiB carry adapter: {adapter:?}");
        for (wide, lanes) in [(false, entries.len() - 2), (true, entries.len())] {
            let (shader, _) = build(wide, descriptor).expect("directory shader lowers");
            let (root, pso) = host.compute_pipeline(&shader).expect("directory pipeline");
            let records = directory_records(&entries[..lanes]);
            let ud = user_data_constants(&shader, [0; 4], 0, 2, 0);
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![
                            (0, Root::Constants(ud)),
                            (1, Root::Uav(super::MEMORY_FAULTS)),
                            (2, Root::DeviceSrv(directory.clone())),
                        ],
                        views: &[
                            View::DeviceRawSrv(&records),
                            View::RawUav(4 * THREADS),
                            View::NullRawUav,
                            View::RawUavData(&arena),
                        ],
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .expect("directory dispatch");
            for (t, (got, want)) in out[0].chunks_exact(4).zip(&expected[..lanes]).enumerate() {
                let (_, pointer, _) = entries[t];
                assert_eq!(
                    got, want,
                    "{adapter:?}, wide={wide}, lane {t}, pointer {pointer:#x}"
                );
            }
            let faults = faulted_pages(&out[1]);
            assert!(faults.is_empty(), "{adapter:?}, wide={wide}: unexpected faults {faults:#x?}");
        }
        ran += 1;
    }
    assert!(ran > 0, "no SM 6.6 device available");
}

/// Real runtime directory path (`Lower::memory` with `SLOT_MEMORY`,
/// `SLOT_FAULTS`, and `USER_MEMORY_BASE`, without `TEST_MEMORY`):
/// verifies multi-arena reads, 128-byte `box32` nodes straddling a 64 KiB
/// page boundary (mapped+mapped across two arenas, mapped+unmapped,
/// unmapped+mapped), and that masked-out lanes and `kind` 6/7 (`NONE`)
/// produce no spurious faults.
#[test]
fn real_directory_memory_path_handles_cross_page_unmapped_and_no_spurious_faults() {
    let (shader, _) = build(true, WHOLE).expect("lowers through the real directory");
    assert!(shader.root_srvs.contains(&super::SLOT_MEMORY));
    assert!(shader.root_uavs.contains(&super::SLOT_FAULTS));

    const PAGE: u64 = 1 << 16;
    const UNIT: [f32; 6] = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
    let moved =
        |b: [f32; 6], by: [f32; 3]| -> [f32; 6] { std::array::from_fn(|k| b[k] + by[k % 3]) };

    // Arena 1 (copy 1): 2 pages (128 KiB).
    // - Page A (page 0x20, offset 0): standard nodes + cross-page box32 first half at 0xFFC0.
    // - Page C (page 0x22, offset 0x10000): cross-page box32 first half at 0xFFC0 whose second
    //   page (0x23) is UNMAPPED.
    let mut arena1 = vec![0u32; (2 * PAGE / 4) as usize];
    // Arena 2 (copy 2): 2 pages (128 KiB), marked MEMORY_READ_ONLY to verify reads still work.
    // - Page B (page 0x21, offset 0): cross-page box32 second half at 0x0000.
    // - Page E (page 0x25, offset 0x10000): second half of a box32 whose first page (0x24) is UNMAPPED.
    let mut arena2 = vec![0u32; (2 * PAGE / 4) as usize];

    let lined = [
        moved(UNIT, [30.0, 0.0, 0.0]),
        moved(UNIT, [10.0, 0.0, 0.0]),
        moved(UNIT, [10.0, 5.0, 0.0]),
        moved(UNIT, [20.0, 0.0, 0.0]),
    ];
    let ids = [0x105, 0x20c, 0x304, 0x400];
    let box32_node = nodes::box32(ids, lined);
    let box16_node = nodes::box16(ids, lined);
    let tri_verts = [[0.0, 2.0, 5.0], [2.0, -2.0, 5.0], [-2.0, -2.0, 5.0]];
    let tri_node = nodes::triangles(&tri_verts, 9);

    // Same-page nodes in Page A (page 0x20, base 0x20_0000):
    // offset 0x000: box32, offset 0x080: box16, offset 0x0c0: triangle
    arena1[0..28].copy_from_slice(&box32_node);
    arena1[0x80 / 4..0x80 / 4 + 16].copy_from_slice(&box16_node[..16]);
    arena1[0xc0 / 4..0xc0 / 4 + 16].copy_from_slice(&tri_node[..16]);

    // Cross-page box32 #1: first 64 bytes at end of Page A (0x20_FFC0, arena1),
    // second 64 bytes at start of Page B (0x21_0000, arena2).
    let cross1_off = 0xFFC0 / 4;
    arena1[cross1_off..cross1_off + 16].copy_from_slice(&box32_node[..16]);
    arena2[0..12].copy_from_slice(&box32_node[16..28]);

    // Cross-page box32 #2: first 64 bytes at end of Page C (0x22_FFC0, arena1),
    // second 64 bytes in Page 0x23 which is UNMAPPED.
    let cross2_off = (PAGE as usize + 0xFFC0) / 4;
    arena1[cross2_off..cross2_off + 16].copy_from_slice(&box32_node[..16]);

    // Cross-page 2-child box32 (children 2 and 3 are NONE): first 64 bytes at
    // end of Page F (0x26_FFC0, arena1 offset 0x20000), second page 0x27 UNMAPPED.
    // Because children 2-3 are NONE, the second half is skipped and page 0x27 must not fault.
    arena1.resize((3 * PAGE / 4) as usize, 0);
    let box32_2child = nodes::box32([0x105, 0x20c, bvh::NONE, bvh::NONE], lined);
    let cross2child_off = (2 * PAGE as usize + 0xFFC0) / 4;
    arena1[cross2child_off..cross2child_off + 16].copy_from_slice(&box32_2child[..16]);

    // Cross-page box32 #3: first 64 bytes at end of Page 0x24 (UNMAPPED),
    // second 64 bytes at start of Page E (0x25_0000, arena2 offset 0x10000).
    let cross3_second = (PAGE as usize) / 4;
    arena2[cross3_second..cross3_second + 12].copy_from_slice(&box32_node[16..28]);

    // Full directory (MEMORY_PAGES entries) so clamped out-of-range addresses also hit the sink.
    let directory = make_directory(
        super::MEMORY_PAGES,
        &[
            (0x20, 1, 0, false),
            (0x21, 2, 0, true),
            (0x22, 1, PAGE as u32, false),
            (0x25, 2, PAGE as u32, true),
            (0x26, 1, 2 * PAGE as u32, false),
        ],
    );

    let ray_x = Ray::new([0.0; 3], [1.0, 0.0, 0.0], 1000.0);
    let ray_z = Ray::new([0.0; 3], [0.0, 0.0, 1.0], 100.0);

    let ptr_at = |addr: u64, kind: u32| -> u64 { (addr >> 3) | u64::from(kind) };

    // Batch 1: ONLY mapped nodes + masked-out unmapped lanes + active kind 6/7 (NONE) unmapped lanes.
    // Expected: ZERO faults in SLOT_FAULTS!
    let batch1_entries = vec![
        // t0: same-page box32
        (true, ptr_at(0x20 * PAGE, 5), ray_x),
        // t1: same-page box16
        (true, ptr_at(0x20 * PAGE + 0x80, 4), ray_x),
        // t2: same-page triangle hit
        (true, ptr_at(0x20 * PAGE + 0xc0, 0), ray_z),
        // t3: cross-page box32 across arena 1 (page 0x20) + arena 2 (page 0x21, RO)
        (true, ptr_at(0x20 * PAGE + 0xFFC0, 5), ray_x),
        // t4: masked-out lane pointing at unmapped page 0x40 -> MUST NOT FAULT
        (false, ptr_at(0x40 * PAGE, 5), ray_x),
        // t5: active kind 6 (instance) pointing at unmapped page 0x41 -> MUST NOT FAULT
        (true, ptr_at(0x41 * PAGE, 6), ray_z),
        // t6: active kind 7 (procedural) pointing at unmapped page 0x42 -> MUST NOT FAULT
        (true, ptr_at(0x42 * PAGE, 7), ray_z),
        // t7: active bvh::NONE (0xffff_ffff, kind 7) -> MUST NOT FAULT
        (true, u64::from(bvh::NONE), ray_z),
        // t8: cross-page 2-child box32 (page 0x26 mapped, page 0x27 unmapped) ->
        // children 2-3 are NONE so page 0x27 is never read or faulted!
        (true, ptr_at(0x26 * PAGE + 0xFFC0, 5), ray_x),
        // t9: same-page triangle miss (ray beside triangle)
        (true, ptr_at(0x20 * PAGE + 0xc0, 0), Ray::new([3.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0)),
    ];
    let batch1_want = vec![
        bvh::intersect(&box32_node, 5, &ray_x, true),
        bvh::intersect(&box16_node, 4, &ray_x, true),
        bvh::intersect(&tri_node, 0, &ray_z, true),
        bvh::intersect(&box32_node, 5, &ray_x, true),
        BEFORE,
        bvh::MISS.map(f32::to_bits),
        bvh::MISS.map(f32::to_bits),
        bvh::MISS.map(f32::to_bits),
        bvh::intersect(&box32_2child, 5, &ray_x, true),
        bvh::MISS.map(f32::to_bits),
    ];

    // Batch 2: unmapped-page edge cases (cross-page mapped+unmapped, unmapped+mapped, unmapped triangle).
    let batch2_entries = vec![
        // t0: cross-page box32 #2 (page 0x22 mapped, page 0x23 unmapped):
        // children 0 (x=30, 0x105) and 1 (x=10, 0x20c) are in first half and hit;
        // children 2 and 3 are in unmapped second half -> NONE. Sorted: [0x20c, 0x105, NONE, NONE].
        (true, ptr_at(0x22 * PAGE + 0xFFC0, 5), ray_x),
        // t1: cross-page box32 #3 (page 0x24 unmapped, page 0x25 mapped): all NONE.
        (true, ptr_at(0x24 * PAGE + 0xFFC0, 5), ray_x),
        // t2: unmapped triangle (page 0x50): MISS.
        (true, ptr_at(0x50 * PAGE, 0), ray_z),
    ];
    let batch2_want = vec![
        [0x20c, 0x105, bvh::NONE, bvh::NONE],
        [bvh::NONE; 4],
        bvh::MISS.map(f32::to_bits),
    ];

    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        if !warp {
            eprintln!("real_directory hardware adapter: {:?}", host.adapter_name());
        }
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        // Heap layout:
        // slot 0: input records SRV
        // slot 1: output UAV (4 * THREADS dwords)
        // slot 2 (= USER_MEMORY_BASE): null raw UAV (copy 0)
        // slot 3 (= USER_MEMORY_BASE + 1): arena 1 (copy 1)
        // slot 4 (= USER_MEMORY_BASE + 2): arena 2 (copy 2)
        let ud = user_data_constants(&shader, [0; 4], 0, 2, 0);

        // Run Batch 1
        let rec1 = directory_records(&batch1_entries);
        let out1 = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![
                        (0, Root::Constants(ud.clone())),
                        (1, Root::Uav(super::MEMORY_FAULTS)),
                        (2, Root::DeviceSrv(directory.clone())),
                    ],
                    views: &[
                        View::DeviceRawSrv(&rec1),
                        View::RawUav(4 * THREADS),
                        View::NullRawUav,
                        View::RawUavData(&arena1),
                        View::RawUavData(&arena2),
                    ],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: batch 1 dispatch failed: {e}"));
        for (t, want) in batch1_want.iter().enumerate() {
            assert_eq!(
                &out1[0][4 * t..4 * t + 4],
                want,
                "{label} batch 1 thread {t}"
            );
        }
        let faults1 = faulted_pages(&out1[1]);
        assert!(
            faults1.is_empty(),
            "{label}: batch 1 recorded spurious page faults for mapped/masked/kind-6-7 lanes: {faults1:#x?}"
        );

        // Run Batch 2
        let rec2 = directory_records(&batch2_entries);
        let out2 = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![
                        (0, Root::Constants(ud)),
                        (1, Root::Uav(super::MEMORY_FAULTS)),
                        (2, Root::DeviceSrv(directory.clone())),
                    ],
                    views: &[
                        View::DeviceRawSrv(&rec2),
                        View::RawUav(4 * THREADS),
                        View::NullRawUav,
                        View::RawUavData(&arena1),
                        View::RawUavData(&arena2),
                    ],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: batch 2 dispatch failed: {e}"));
        for (t, want) in batch2_want.iter().enumerate() {
            assert_eq!(
                &out2[0][4 * t..4 * t + 4],
                want,
                "{label} batch 2 thread {t}"
            );
        }
        let faults2 = faulted_pages(&out2[1]);
        assert!(
            faults2.contains(&0x23) && faults2.contains(&0x24) && faults2.contains(&0x50),
            "{label}: batch 2 expected faults for pages 0x23, 0x24, 0x50; got {faults2:#x?}"
        );
        ran += 1;
    }
    assert!(ran > 0, "no SM 6.6 device available");
}

/// A console wave64 compute program (64 threads, spilled user-data table,
/// cross-half wave OR so it must split into two 32-lane waves on NVIDIA)
/// executing `image_bvh64_intersect_ray` in a partial final group
/// (`USER_DISPATCH_THREADS_X = 45`): lanes 0..44 match the CPU reference and
/// lanes 45..63 (which stay alive for the exchange barriers) must not read
/// title memory or record page faults.
#[test]
fn wave64_bvh64_intersect_runs_on_wave32_hardware_and_respects_partial_groups() {
    let mut w = vec![
        // Spilled user-data table at s[0:1]: read descriptor table offset (dword 0) into s4.
        0x8703_FF01,
        0x0000_00FF, // s_and_b32 s3, s1, 0xff
        0xBE82_0300, // s_mov_b32 s2, s0
        0x3D << 26 | 4 << 6 | 1,
        125 << 25, // s_load_dword s4, s[2:3], 0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202,
        0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0x0 (records SRV)
        0xF408_0302,
        0xFA00_0020, // s_load_dwordx4 s[12:15], s[4:5], 0x20 (output UAV)
        // Pre-seed v1 with an unmapped node pointer (page 0x60, box32) via ALU so
        // trailing partial-group lanes (whose buffer_load is masked by partial_active)
        // still hold a non-zero unmapped address when reaching image_bvh64_intersect_ray.
        0x7E02_02FF,
        ((0x60 * (1u32 << 16)) >> 3) | 5, // v_mov_b32 v1, (0x60_0000 >> 3) | 5
        0x7E04_0280,                      // v_mov_b32 v2, 0
        0x3428_0086,                      // v_lshlrev_b32 v20, 6, v0
        0xE038_1000,
        0x8002_0114, // buffer_load_dwordx4 v[1:4], v20, s[8:11], 0 offen
        0xE038_1010,
        0x8002_0514, // buffer_load_dwordx4 v[5:8], v20, s[8:11], 0 offen offset:16
        0xE038_1020,
        0x8002_0914, // buffer_load_dwordx4 v[9:12], v20, s[8:11], 0 offen offset:32
        // Whole-address-space BVH descriptor in s[24:27].
        0xBE98_0380, // s_mov_b32 s24, 0
        0xBE99_0B81, // s_brev_b32 s25, 1 (BOX_SORT)
        0xBE9A_03C1, // s_mov_b32 s26, -1
        0xBE9B_03FF,
        0x8900_03FF, // s_mov_b32 s27, 0x890003ff
        // image_bvh64_intersect_ray v[13:16], [v1 .. v12], s[24:27]
        0xF19C_9F07,
        0x0006_0D01,
        0x0504_0302,
        0x0908_0706,
        0x000C_0B0A,
        // Cross-half wave OR on v13 (forces two 32-lane waves to stay alive for exchange barriers).
        0x7E0E_030D, // v_mov_b32 v7, v13
    ];
    w.extend_from_slice(&super::wave::WAVE_OR_V7_TO_S7);
    w.extend_from_slice(&[
        0x342A_0084, // v_lshlrev_b32 v21, 4, v0
        0xE078_1000,
        0x8003_0D15, // buffer_store_dwordx4 v[13:16], v21, s[12:15], 0 offen
        0xBF81_0000, // s_endpgm
    ]);

    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 22,
        layout: vec![RootParam {
            root: RootType::Table,
            table_offset_dwords: 0,
            sgprs: 1,
            base_register: 0,
            user_data: 16,
            slot: 0,
        }],
        used: vec![
            UsedResource {
                kind: ResourceType::Srv,
                root: RootType::Table,
                table_offset_dwords: 0,
                lower_bound: 0,
                stride: 4,
                dwords: 4,
                user_data: 16,
                slot: 0,
            },
            UsedResource {
                kind: ResourceType::Uav,
                root: RootType::Table,
                table_offset_dwords: 8,
                lower_bound: 0,
                stride: 4,
                dwords: 4,
                user_data: 16,
                slot: 0,
            },
        ],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 2 << 1,
            threads: [64, 1, 1],
        }),
        code: w.iter().flat_map(|x| x.to_le_bytes()).collect(),
    };

    let p = crate::spill::build(&native).expect("spill::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("resource::track");
    let io = io::raise(&p, &native).expect("io::raise");
    let was = lower::TEST_HOST_WAVE.replace(Some(32));
    let lowered = lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
    });
    lower::TEST_HOST_WAVE.set(was);
    let lowered = lowered.expect("wave64 BVH shader lowers for 32-lane host");
    assert_ne!(lowered.wave_size, Some(64));
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: lowered.dxil,
        user_data: lowered.user_data,
        samplers: lowered.samplers,
        static_samplers: Vec::new(),
        root_cbvs: lowered.root_cbvs,
        root_srvs: lowered.root_srvs,
        root_uavs: lowered.root_uavs,
        root_indexed_views: res.root_indexed_views,
    };
    if let Ok(mut v) = validator::Validator::load(validator::Validator::default_path().as_deref()) {
        let mut copy = shader.dxil.clone();
        v.validate(&mut copy).expect("DXIL validation failed");
    }

    const PAGE: u64 = 1 << 16;
    const UNIT: [f32; 6] = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
    let moved =
        |b: [f32; 6], by: [f32; 3]| -> [f32; 6] { std::array::from_fn(|k| b[k] + by[k % 3]) };
    let lined = [
        moved(UNIT, [30.0, 0.0, 0.0]),
        moved(UNIT, [10.0, 0.0, 0.0]),
        moved(UNIT, [10.0, 5.0, 0.0]),
        moved(UNIT, [20.0, 0.0, 0.0]),
    ];
    let box32_node = nodes::box32([0x105, 0x20c, 0x304, 0x400], lined);
    let mut arena = vec![0u32; (PAGE / 4) as usize];
    arena[0..28].copy_from_slice(&box32_node);
    let directory = make_directory(0x80, &[(0x20, 1, 0, false)]);

    let ray_x = Ray::new([0.0; 3], [1.0, 0.0, 0.0], 1000.0);
    let active_count = 45usize;
    let mut records = vec![0u32; 64 * (RECORD / 4) as usize];
    for t in 0..64usize {
        let r = &mut records[t * (RECORD / 4) as usize..][..13];
        // Lanes 0..44 point at mapped page 0x20; trailing lanes 45..63 point at unmapped page 0x60.
        let addr = if t < active_count { 0x20 * PAGE } else { 0x60 * PAGE };
        let pointer = (addr >> 3) | 5;
        r[0] = pointer as u32;
        r[1] = (pointer >> 32) as u32;
        r[2] = ray_x.extent.to_bits();
        for a in 0..3 {
            r[3 + a] = ray_x.origin[a].to_bits();
            r[6 + a] = ray_x.direction[a].to_bits();
            r[9 + a] = ray_x.inverse[a].to_bits();
        }
    }

    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Ok(host) = Host::new(false) else {
        eprintln!("no SM 6.6 hardware device; skipped");
        return;
    };
    eprintln!("wave64_bvh64 hardware adapter: {:?}", host.adapter_name());
    let (root, pso) = host
        .compute_pipeline(&shader)
        .unwrap_or_else(|e| panic!("hardware compute_pipeline failed: {e}"));
    let ud = user_data_constants(&shader, [0; 4], 0, 2, active_count as u32);
    let out = host
        .dispatch(
            &root,
            &pso,
            &Dispatch {
                roots: vec![
                    (0, Root::Constants(ud)),
                    (1, Root::Uav(super::MEMORY_FAULTS)),
                    (2, Root::DeviceSrv(directory)),
                ],
                views: &[
                    View::DeviceRawSrv(&records),
                    View::RawUav(4 * 64),
                    View::NullRawUav,
                    View::RawUavData(&arena),
                ],
                samplers: &[],
                groups: [1, 1, 1],
            },
        )
        .unwrap_or_else(|e| panic!("hardware dispatch failed: {e}"));

    let want = bvh::intersect(&box32_node, 5, &ray_x, true);
    for t in 0..active_count {
        assert_eq!(&out[0][4 * t..4 * t + 4], &want, "active thread {t}");
    }
    for t in active_count..64 {
        assert_eq!(&out[0][4 * t..4 * t + 4], &[0; 4], "inactive trailing thread {t}");
    }
    let faults = faulted_pages(&out[1]);
    assert!(
        faults.is_empty(),
        "trailing lanes 45..63 of partial wave64 group recorded spurious faults: {faults:#x?}"
    );
}

/// GPU timestamp benchmark for `image_bvh64_intersect_ray` through the real
/// `SLOT_MEMORY` directory + VRAM arenas on the hardware GPU.
/// Run with: `cargo test -p victory-shader --features host -- bench_bvh64_intersect_on_gpu --ignored --nocapture`
#[test]
#[ignore]
fn bench_bvh64_intersect_on_gpu() {
    const GROUP_THREADS: u32 = 64;
    const GROUPS: u32 = 512; // 32,768 threads
    const TOTAL_THREADS: usize = (GROUP_THREADS * GROUPS) as usize;
    const ITERS: u32 = 64; // 2,097,152 intersect calls per dispatch
    const PAGE: u64 = 1 << 16;

    // Multi-group loop shader: each thread loads a 64-byte record (ptr_lo, ptr_hi, ray[10],
    // active_flag, ptr_step, ptr_mask, unused), loops `s0` iterations calling
    // `image_bvh64_intersect_ray` while stepping the node pointer across the arena, XOR-accumulates
    // all 4 result dwords into v[13:16], and writes v[13:16] to the output UAV.
    let words: Vec<u32> = vec![
        0xBE86_0305, // s_mov_b32 s6, s5 (group_id.x)
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202,
        0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0x0 (records SRV)
        0xF408_0302,
        0xFA00_0020, // s_load_dwordx4 s[12:15], s[4:5], 0x20 (output UAV)
        0x7E28_0206, // v_mov_b32 v20, s6
        0x3428_2886, // v_lshlrev_b32 v20, 6, v20 (group_id.x * 64)
        0x4A28_2900, // v_add_nc_u32 v20, v0, v20 (global_tid)
        0x342A_2886, // v_lshlrev_b32 v21, 6, v20 (global_tid * 64)
        0xE038_1000,
        0x8002_0115, // buffer_load_dwordx4 v[1:4], v21, s[8:11], 0 offen
        0xE038_1010,
        0x8002_0515, // buffer_load_dwordx4 v[5:8], v21, s[8:11], 0 offen offset:16
        0xE038_1020,
        0x8002_0915, // buffer_load_dwordx4 v[9:12], v21, s[8:11], 0 offen offset:32
        0xE038_1030,
        0x8002_1115, // buffer_load_dwordx4 v[17:20], v21, s[8:11], 0 offen offset:48 (v17=active, v18=step, v19=base_lo)
        // Recompute global_tid*16 in v22 for output store before clobbering v20.
        0x7E2C_0206, // v_mov_b32 v22, s6
        0x342C_2C86, // v_lshlrev_b32 v22, 6, v22
        0x4A2C_2D00, // v_add_nc_u32 v22, v0, v22
        0x342C_2C84, // v_lshlrev_b32 v22, 4, v22 (global_tid * 16)
        // Accumulators v[13:16] = 0
        0x7E1A_0280, // v_mov_b32 v13, 0
        0x7E1C_0280, // v_mov_b32 v14, 0
        0x7E1E_0280, // v_mov_b32 v15, 0
        0x7E20_0280, // v_mov_b32 v16, 0
        // Whole-memory BVH descriptor in s[24:27]
        0xBE98_0380, // s_mov_b32 s24, 0
        0xBE99_0B81, // s_brev_b32 s25, 1 (BOX_SORT)
        0xBE9A_03C1, // s_mov_b32 s26, -1
        0xBE9B_03FF,
        0x8900_03FF, // s_mov_b32 s27, 0x890003ff
        // Loop counter s22 = s0
        0xBE96_0300, // s_mov_b32 s22, s0
        // --- loop header ---
        // Initialize temp result v[23:26] = BEFORE
        0x7E2E_0281, // v_mov_b32 v23, 1
        0x7E30_0282, // v_mov_b32 v24, 2
        0x7E32_0283, // v_mov_b32 v25, 3
        0x7E34_0284, // v_mov_b32 v26, 4
        // Mask inactive lanes per iteration (like traversal termination)
        0xBE94_037E, // s_mov_b32 s20, exec_lo
        0x7DAA_2280, // v_cmpx_ne_u32 exec_lo, 0, v17
        // image_bvh64_intersect_ray v[23:26], [v1 .. v12], s[24:27]
        0xF19C_9F07,
        0x0006_1701,
        0x0504_0302,
        0x0908_0706,
        0x000C_0B0A,
        0xBEFE_0314, // s_mov_b32 exec_lo, s20
        // Accumulate XOR
        0x3A1A_2F0D, // v_xor_b32 v13, v13, v23
        0x3A1C_310E, // v_xor_b32 v14, v14, v24
        0x3A1E_330F, // v_xor_b32 v15, v15, v25
        0x3A20_3510, // v_xor_b32 v16, v16, v26
        // Step pointer low dword by v18 (in 8-byte units, preserving kind bits in low 3 bits)
        0x4A02_2501, // v_add_nc_u32 v1, v1, v18
        // s22 -= 1; if s22 > 0 branch back
        0x8096_8116, // s_sub_u32 s22, s22, 1
        0xBF08_8016, // s_cmp_gt_u32 s22, 0
        0xBF85_FFEC, // s_cbranch_scc1 -20 (back to v_mov_b32 v23, 1: 20 dwords)
        // Store accumulators
        0xE078_1000,
        0x8003_0D16, // buffer_store_dwordx4 v[13:16], v22, s[12:15], 0 offen
        0xBF81_0000, // s_endpgm
    ];

    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 28,
        layout: vec![
            RootParam {
                root: RootType::Constants,
                table_offset_dwords: 0,
                sgprs: 4,
                base_register: 0,
                user_data: 0,
                slot: 0,
            },
            RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 0,
                user_data: 4,
                slot: 1,
            },
        ],
        used: vec![
            UsedResource {
                kind: ResourceType::Srv,
                root: RootType::Table,
                table_offset_dwords: 0,
                lower_bound: 0,
                stride: 4,
                dwords: 4,
                user_data: 4,
                slot: 1,
            },
            UsedResource {
                kind: ResourceType::Uav,
                root: RootType::Table,
                table_offset_dwords: 8,
                lower_bound: 0,
                stride: 4,
                dwords: 4,
                user_data: 4,
                slot: 1,
            },
        ],
        io: StageIo::Cs(ComputeIo {
            rsrc2: (5 << 1) | (1 << 7), // s0..s4 user, s5 = group_id.x
            threads: [GROUP_THREADS, 1, 1],
        }),
        code: words.iter().flat_map(|x| x.to_le_bytes()).collect(),
    };

    let (shader, _) = build_compute_shader(&native).expect("bench shader lowers");

    // Build an 8 MiB VRAM arena (128 pages of 64 KiB) populated with synthetic BVH nodes
    // at 128-byte slots (65,536 slots), mapped at title pages 0x100..0x180.
    // Generate 1,024 distinct random nodes and tile them across the 8 MiB arena so setup
    // finishes in milliseconds while preserving the full 8 MiB working set.
    const ARENA_PAGES: usize = 128;
    const NODE_STRIDE_BYTES: usize = 128;
    const NODE_COUNT: usize = (ARENA_PAGES * PAGE as usize) / NODE_STRIDE_BYTES;
    const UNIQUE_NODES: usize = 1024;
    let mut arena_box32 = vec![0u32; (ARENA_PAGES * PAGE as usize) / 4];
    let mut arena_box32_2child = vec![0u32; (ARENA_PAGES * PAGE as usize) / 4];
    let mut arena_box16 = vec![0u32; (ARENA_PAGES * PAGE as usize) / 4];
    let mut arena_tri = vec![0u32; (ARENA_PAGES * PAGE as usize) / 4];
    let mut g = Numbers(0xB07_5070);
    for idx in 0..UNIQUE_NODES {
        let base_w = idx * (NODE_STRIDE_BYTES / 4);
        let boxes: [[f32; 6]; 4] = std::array::from_fn(|_| {
            let c = g.vector(8.0);
            let h = g.vector(2.0).map(|x| x.abs() + 0.25);
            std::array::from_fn(|k| c[k % 3] + if k < 3 { -h[k] } else { h[k - 3] })
        });
        let children: [u32; 4] =
            std::array::from_fn(|k| ((idx as u32 + 1) * 32 + k as u32 * 8) | 5);
        let children2 = [children[0], children[1], bvh::NONE, bvh::NONE];
        let b32 = nodes::box32(children, boxes);
        let b32_2 = nodes::box32(children2, boxes);
        let b16 = nodes::box16(children, boxes);
        arena_box32[base_w..base_w + 28].copy_from_slice(&b32);
        arena_box32_2child[base_w..base_w + 28].copy_from_slice(&b32_2);
        arena_box16[base_w..base_w + 16].copy_from_slice(&b16[..16]);

        let c = g.vector(6.0);
        let v: [[f32; 3]; 5] = std::array::from_fn(|_| {
            let d = g.vector(3.0);
            [c[0] + d[0], c[1] + d[1], c[2] + d[2]]
        });
        let tri = nodes::triangles(&v, 9 | (2 << 8) | (4 << 16) | (9 << 24));
        arena_tri[base_w..base_w + 16].copy_from_slice(&tri[..16]);
    }
    let tile_words = UNIQUE_NODES * (NODE_STRIDE_BYTES / 4);
    for tile in 1..(NODE_COUNT / UNIQUE_NODES) {
        let dst = tile * tile_words;
        arena_box32.copy_within(0..tile_words, dst);
        arena_box32_2child.copy_within(0..tile_words, dst);
        arena_box16.copy_within(0..tile_words, dst);
        arena_tri.copy_within(0..tile_words, dst);
    }

    // Map primary arenas + 8 distinct box32 arenas (copies 4..11) + 2-child box32 (copy 12).
    let mut mappings = Vec::new();
    for p in 0..ARENA_PAGES as u32 {
        let off = p * PAGE as u32;
        mappings.push((0x100 + p, 1, off, false));
        mappings.push((0x200 + p, 2, off, false));
        mappings.push((0x300 + p, 3, off, false));
        for a in 0..8u32 {
            mappings.push((0x1000 + a * 0x100 + p, 4 + a, off, false));
        }
        mappings.push((0x2000 + p, 12, off, false));
    }
    let directory = make_directory(super::MEMORY_PAGES, &mappings);

    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = Host::new(false).expect("SM 6.6 hardware GPU required");
    let adapter = host.adapter_name().unwrap_or_else(|| "unknown".into());
    eprintln!("=== RTX GPU Benchmark Adapter: {adapter} (DXIL {} bytes) ===", shader.dxil.len());
    let (root, pso) = host.compute_pipeline(&shader).expect("compute_pipeline");
    let ud = user_data_constants(&shader, [ITERS, 0, 0, 0], 0, 2, 0);

    #[derive(Clone, Copy)]
    enum Workload {
        Box32Only,
        Box32TwoChild,
        Box32L1Coherent,
        Box32MultiArena,
        Box16Only,
        Box16L1Coherent,
        MixedBox16Box32,
        TriangleOnly,
        TriangleL1Coherent,
        MixedTraversal,
        MixedTraversalPcFaults,
    }

    let workloads = [
        ("box32_only", Workload::Box32Only),
        ("box32_2child", Workload::Box32TwoChild),
        ("box32_l1_coherent", Workload::Box32L1Coherent),
        ("box32_8arenas", Workload::Box32MultiArena),
        ("box16_only", Workload::Box16Only),
        ("box16_l1_coherent", Workload::Box16L1Coherent),
        ("mixed_box16_box32", Workload::MixedBox16Box32),
        ("triangle_only", Workload::TriangleOnly),
        ("tri_l1_coherent", Workload::TriangleL1Coherent),
        ("mixed_traversal", Workload::MixedTraversal),
        ("mixed_pc_faults", Workload::MixedTraversalPcFaults),
    ];

    let step_nodes = 17usize; // prime stride across 65,536 nodes -> hits many pages/cache lines
    let step_ptr_units = (step_nodes * NODE_STRIDE_BYTES / 8) as u32;

    for (name, wl) in workloads {
        let mut rg = Numbers(0x1234_5678);
        let mut records = vec![0u32; TOTAL_THREADS * 16];
        let mut sample_specs = Vec::new();
        let is_coherent = matches!(
            wl,
            Workload::Box32L1Coherent | Workload::Box16L1Coherent | Workload::TriangleL1Coherent
        );
        let pc_faults = matches!(wl, Workload::MixedTraversalPcFaults);
        let sort = true;
        for t in 0..TOTAL_THREADS {
            let lane = t & 31;
            let (base_page, kind, active, step) = match wl {
                Workload::Box32Only | Workload::Box32L1Coherent => {
                    (0x100u64, 5u32, true, step_ptr_units)
                }
                Workload::Box32TwoChild => (0x2000u64, 5u32, true, step_ptr_units),
                Workload::Box32MultiArena => (0x1000u64 + ((lane as u64 & 7) * 0x100), 5u32, true, step_ptr_units),
                Workload::Box16Only | Workload::Box16L1Coherent => (0x200u64, 4u32, true, step_ptr_units),
                Workload::MixedBox16Box32 => {
                    if lane & 1 == 0 {
                        (0x100, 5, true, step_ptr_units)
                    } else {
                        (0x200, 4, true, step_ptr_units)
                    }
                }
                Workload::TriangleOnly | Workload::TriangleL1Coherent => {
                    (0x300, (lane & 3) as u32, true, step_ptr_units)
                }
                Workload::MixedTraversal | Workload::MixedTraversalPcFaults => match lane % 16 {
                    0..=5 => (0x100, 5, true, step_ptr_units),
                    6..=9 => (0x200, 4, true, step_ptr_units),
                    10..=13 => (0x300, (lane & 3) as u32, true, step_ptr_units),
                    14 => (0x500, 7, true, 0), // unmapped kind 7 (NONE/procedural)
                    _ => (0x500, 5, false, 0), // masked-out finished ray
                },
            };
            // Keep all ITERS steps inside the 65,536-node arena (wrap start slot safely).
            let max_start = NODE_COUNT - (ITERS as usize * step_nodes) - 1;
            let start_slot = if is_coherent {
                // Coherent warp: all 32 lanes read the exact same node each iteration (primary rays).
                ((t / 32) * 7) % max_start
            } else {
                (t * 31) % max_start
            };
            let addr = base_page * PAGE + (start_slot * NODE_STRIDE_BYTES) as u64;
            let ptr = (addr >> 3) | u64::from(kind);
            let origin = rg.vector(10.0);
            let dir = rg.vector(1.0);
            let ray = Ray::new(origin, dir, 50.0);

            let r = &mut records[t * 16..t * 16 + 16];
            r[0] = ptr as u32;
            r[1] = (ptr >> 32) as u32;
            r[2] = ray.extent.to_bits();
            for a in 0..3 {
                r[3 + a] = ray.origin[a].to_bits();
                r[6 + a] = ray.direction[a].to_bits();
                r[9 + a] = ray.inverse[a].to_bits();
            }
            r[12] = u32::from(active);
            r[13] = step;
            if t < 64 {
                sample_specs.push((t, base_page, start_slot, kind, active, step, ray));
            }
        }

        let faults_root = if pc_faults {
            Root::SharedUav(super::MEMORY_FAULTS)
        } else {
            Root::Uav(super::MEMORY_FAULTS)
        };
        let (out, mut times_ms) = host
            .dispatch_timed(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![
                        (0, Root::Constants(ud.clone())),
                        (1, faults_root),
                        (2, Root::DeviceSrv(directory.clone())),
                    ],
                    views: &[
                        View::DeviceRawSrv(&records),
                        View::RawUav(4 * TOTAL_THREADS as u32),
                        View::NullRawUav,
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box16),
                        View::RawUavData(&arena_tri),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32),
                        View::RawUavData(&arena_box32_2child),
                    ],
                    samplers: &[],
                    groups: [GROUPS, 1, 1],
                },
                10,
                40,
            )
            .unwrap_or_else(|e| panic!("{name}: dispatch_timed failed: {e}"));

        // Verify bit-for-bit output against CPU reference for the first 64 threads (2 warps).
        for &(t, base_page, start_slot, kind, active, step, ray) in &sample_specs {
            let mut acc = [0u32; 4];
            let mut slot = start_slot;
            for _ in 0..ITERS {
                let res = if !active {
                    BEFORE
                } else if kind >= 6 {
                    bvh::MISS.map(f32::to_bits)
                } else {
                    let arena = if base_page == 0x2000 {
                        &arena_box32_2child
                    } else {
                        match kind {
                            5 => &arena_box32,
                            4 => &arena_box16,
                            _ => &arena_tri,
                        }
                    };
                    let base_w = slot * (NODE_STRIDE_BYTES / 4);
                    let mut node = [0u32; 28];
                    node.copy_from_slice(&arena[base_w..base_w + 28]);
                    bvh::intersect(&node, kind, &ray, sort)
                };
                for k in 0..4 {
                    acc[k] ^= res[k];
                }
                if step != 0 {
                    slot += step_nodes;
                }
            }
            assert_eq!(
                &out[0][4 * t..4 * t + 4],
                &acc,
                "{name}: bit mismatch at thread {t}"
            );
        }

        times_ms.sort_by(|a, b| a.total_cmp(b));
        let median = (times_ms[(times_ms.len() - 1) / 2] + times_ms[times_ms.len() / 2]) / 2.0;
        let p95 = times_ms[(95 * times_ms.len()).div_ceil(100) - 1];
        let min = times_ms[0];
        let mean = times_ms.iter().sum::<f64>() / times_ms.len() as f64;
        let total_calls = (TOTAL_THREADS as f64) * (ITERS as f64);
        let m_per_s = (total_calls / 1e6) / (median / 1e3);
        let faults = faulted_pages(&out[1]);
        let mut hash = blake3::Hasher::new();
        for word in &out[0] {
            hash.update(&word.to_le_bytes());
        }
        let output = hash.finalize().to_hex();
        eprintln!(
            "BENCH {name:18} | median {median:8.4} ms | p95 {p95:8.4} ms | min {min:8.4} ms | mean {mean:8.4} ms | {m_per_s:7.1} M tests/s | faults={} | output={output}",
            faults.len()
        );
    }
}
