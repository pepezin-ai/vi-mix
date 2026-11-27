# État de la branche `rust/full`

Version anglaise : [../rust-full-status.md](../rust-full-status.md). Plan : [architecture-rust.md](architecture-rust.md).

## Base : main (VIctory Neptune)

Cette branche contient désormais `origin/main` (fusion de la PR #277 et de tout ce qui la précède) : `third_party/victory` est l'arbre Neptune de main, avec deux de nos correctifs réappliqués par-dessus (VICTORY_KEYBOARD et VICTORY_WINDOW_MODE=borderless, commit b27dbae4), puis les changements de rust/full sur xmem (EraFields) et xasync. `anyxboxx run` donne à VIctory le jeu de réglages du lanceur de main : build release, jeu Neptune (46 variables : 39 mesurées le 25 novembre 2026, plus les trois options de la PR #358, les trois de la PR #383 et `VICTORY_HEAP_SCAN_ALT`, mesurées le 26 novembre), `-Quality` (`--quality`), `--override`, `--auto-a`, `--frametime-dump`, `--warm`, `--precompile`, `--diag`/`--dred`. La copie de référence du script de main est `tests/fixtures/lancer_victory_neptune.ps1` ; `tests/test_anyxboxx_run.py` et `tests/test_lanceur_victory.py` comparent les réglages Rust à elle. Le lanceur graphique démarre le jeu détaché (journal sous `%LOCALAPPDATA%\AnyXboxX\logs`), se ferme dès que le jeu tourne sauf si « Garder le lanceur ouvert » est coché, et réchauffe le cache des shaders automatiquement quand le marqueur de ce build et de cet environnement manque ou est périmé (`warm-state.txt`).

Porté depuis : les formes f16 packés, FMA à précision mixte et `v_dot2_f32_f16` de VOP3P, ainsi que `v_pk_fmac_f16` (VOP2 et VOP3), sont dans `crates/anyxboxx-rdna2-ref` (`half.rs`, `pkf16.rs`, `pkf16_mix.rs`) ; les tests de parité de `tests/test_rdna2_ref_parity.py` repassent (38 sur 38 ; 1808 tests pytest réussis le 2026-11-26).

## La tester en local (Windows)

```powershell
git fetch origin
git checkout rust/full
git submodule update --init
cargo build --release --workspace

# Ligne de commande (a remplacé tools\lancer_victory.ps1 : mêmes options, même environnement) :
.\target\release\anyxboxx.exe run --game-dir "C:\path\to\your\game" --xg-dll "C:\path\to\your\game\dll\xg_xs.dll" --precompile

# Lanceur graphique (double-clic sur target\release\anyxboxx-launcher.exe, ou) :
.\target\release\anyxboxx-launcher.exe
```

- `anyxboxx run` compile d'abord VIctory (`cargo build --profile diag -p victory --features diagnostics`), comme le script : le premier lancement prend plusieurs minutes. `--precompile` remplit le cache de shaders avant le jeu (environ 4,5 minutes la première fois, mesuré le 25/11/2026). Le premier lancement à froid peut planter pendant la compilation des pipelines ; relancer une fois le cache rempli.
- Les noms d'options PowerShell sont acceptés aussi (`--GameDir`, `--XgDll`, `--XgMode`, `--Precompile`, `--Dred`, `--NoTunedSettings`, `--SkipPipelines`, `--BatchCardRehash`). `anyxboxx run --help` les liste.
- Un seul jeu à la fois : fermer d'abord un jeu déjà lancé. Le lanceur graphique le vérifie lui-même.

### Linux (Proton)

La compilation native sous Linux n'est pas prise en charge : VIctory, que l'espace racine utilise via `victory-hle`, ne compile que pour Windows (`victory-gpu` utilise sans condition ses modules `#[cfg(windows)]`). Compiler les deux espaces pour Windows depuis Linux, avec un CRT et un SDK MSVC sous Linux (par exemple [xwin](https://github.com/Jake-Shadle/xwin)) et `lld-link` :

```bash
cargo build --release --workspace --target x86_64-pc-windows-msvc \
    --exclude anyxboxx-kernelx --exclude anyxboxx-d3d12-xs
(cd third_party/victory && cargo build --profile diag -p victory --features diagnostics --target x86_64-pc-windows-msvc)
```

- `kernelx` et `d3d12_xs` ont besoin du `lib.exe` de MSVC pour produire le `.exp` de leur table de redirections (`anyxboxx-forwarders::link`) ; `llvm-lib /DEF` n'écrit que le `.lib`. Pas bloquant : le jeu passe par VIctory, qui ne charge pas ces DLL.
- Il n'y a pas de cargo dans Proton : `anyxboxx.exe run` prend le VIctory compilé depuis Linux avec `--victory-exe` et saute la compilation (le dépôt n'est alors pas nécessaire). Le lancer avec le `wine` de Proton et le préfixe Wine de son choix, chemins écrits en `Z:\…` :

```bash
VKD3D_CONFIG=descriptor_heap "$PROTON/files/bin/wine" target/x86_64-pc-windows-msvc/release/anyxboxx.exe run \
    --victory-exe 'Z:\…\third_party\victory\target\x86_64-pc-windows-msvc\diag\victory.exe' \
    --game-dir 'Z:\…\votre dossier du jeu'
```

- `anyxboxx run` applique le jeu de réglages Windows (`TUNED`), pas le profil `linux-amd`, que seul le lanceur graphique lit. Wine transmet l'environnement Linux : exporter `VKD3D_CONFIG=descriptor_heap` (le tas de vues du titre dépasse le plafond par défaut de vkd3d-proton) et, pour les valeurs de `linux-amd`, ajouter `--no-tuned-settings` et les exporter soi-même.
- Le lanceur graphique accepte aussi `--victory-exe <chemin>` (ou `--victory-exe=<chemin>`) pour lancer un `victory.exe` précompilé depuis ses pages Jouer et Shaders sans compilation cargo ni dépôt local.

### Lanceur graphique (`crates/anyxboxx-launcher`)

Installation et utilisation : [lanceur.md](lanceur.md). Exécutable précompilé : [releases du lanceur](https://github.com/LooperSalty/AnyXboxX/releases?q=launcher&expanded=true).

Il est fait avec egui via eframe 0.34 : du Rust pur, sans liaison C++ ni installeur. La fenêtre compte quatre pages avec des barres de progression, un journal et quelques boîtes de dialogue, qu'egui (mode immédiat) dessine en quelques centaines de lignes. Le Win32 brut aurait demandé des boucles de messages et une mise en page écrites à la main pour les mêmes pages. La logique vit dans des modules simples que la fenêtre ne fait qu'appeler : les 190 tests n'ouvrent jamais de fenêtre.

Style : un dégradé de crépuscule et des cartes translucides (`theme.rs`, calcul des couleurs testé pour le niveau AA du WCAG, y compris le texte sur le voile de l'image mélangé en lumière linéaire), une seule échelle d'espacement sur une grille de 8 points, une échelle typographique et un jeu de rayons (`tokens.rs`), et des contrôles dessinés à la main avec survol, appui et anneau de focus clavier (`widgets.rs`). Les images de l'écran d'accueil ne sont jamais versionnées : elles sont lues dans `ANYXBOXX_LAUNCHER_SPLASH` (un fichier ou un dossier), sinon dans les PNG et JPEG de `%LOCALAPPDATA%\AnyXboxX\launcher\splash\` (plusieurs images défilent en diaporama avec un fondu), sinon `%LOCALAPPDATA%\AnyXboxX\launcher\splash.png` ; sans image, le dégradé et le titre sont dessinés. La page Accueil l'affiche en grand avec un zoom lent et une parallaxe qui suit le pointeur ; quand PLAY compile VIctory ou réchauffe les shaders, un écran de lancement plein fenêtre (`ui_launch.rs`) montre les étapes (Building VIctory, Preloading shaders, Starting the game), une barre alimentée par les lignes de progression de VIctory, le temps écoulé et le temps restant (`launch_progress.rs`, testé), et Cancel, qui tue l'étape en cours (`job.rs`). Les lignes « À propos » donnent la version et le commit git (intégré par `build.rs`). `--page <home|setup|folder|shaders|play>` ouvre une page ; les builds debug acceptent aussi `--preview-launch` pour montrer l'écran de lancement sans rien exécuter.

| Page | Ce qu'elle fait | Limites |
| --- | --- | --- |
| Installation | Vérifie la chaîne Rust, `d3d12.dll`, le runtime VC++ (présent, version non vérifiée), `dxc` du SDK Windows et le GPU via DXGI. Liens vers les pages officielles seulement ; rien n'est téléchargé. | La version du pilote affiche « inconnue » : le dépôt ne fixe encore aucun minimum. |
| Fichiers | Dossier du jeu (chemin saisi) : présence, fichiers vides, bornes des sections PE (exe ou DLL tronqué), bornes de la table d'en-tête RPF7, et un manifeste SHA-256 des fichiers requis et des `.rpf` de premier niveau, calculé en local. Rien n'est copié ni envoyé. | Il n'y a pas de hachés de référence : le manifeste est un relevé, pas une preuve. Une archive coupée après des tables intactes n'est pas détectée. La disposition RPF7 n'a pas été vérifiée sur un vrai fichier. |
| Shaders | `victory translate --cache --pipelines --used-pipelines … --jobs N` sur `shaders.rpf`, l'exe et ses deux DLL seulement, avec l'environnement du jeu (la clé du cache en dépend). La progression est lue dans les lignes « N of M containers (x%) » de VIctory. Refusé pendant que `victory.exe` ou le jeu tourne. | |
| Jouer | Profil choisi selon le système et le GPU (`profiles.toml`), surcharges dans le panneau avancé, `xg_xs.dll` facultative (real ou compare), compilation puis lancement par les mêmes étapes qu'`anyxboxx run`, emplacement des sauvegardes par titre (`<data>\<TitleId>\saves`), nettoyage du cache de shaders (demande confirmation ; seul un dossier nommé `cache` peut être supprimé). | VIctory n'écrit les i/s que dans le titre de sa fenêtre : pas de ligne d'i/s à afficher. |

Profils (`crates/anyxboxx-launcher/profiles.toml`, un test chacun) :

- **Windows + NVIDIA :** ton jeu de réglages (les six valeurs `TUNED` d'`anyxboxx run`, sans DRED, sans relais). Mesuré : 2-3 i/s sur `main` après la PR #199, 5-7 i/s sur le fork à `80d58f8`. Mettre les interrupteurs de la PR #199 à `0` n'a pas changé le chiffre de `main` : le profil les laisse aux valeurs par défaut de VIctory.
- **Linux (Proton) + AMD :** les valeurs de l'essai de la PR #199 (RX 9070 XT, RADV, GE-Proton11-12) :
  - `SHADER_LOOP_LIMIT=256`, `LATE_WAIT=0`, `WIDE_BACK=0`, `PIPELINE_WAIT_MS=50`
  - `TEXTURE_BUDGET_MB=8192` (pour une carte de 16 Go), `TEXTURE_NO_WAIT=1`
  - `CARD_BUFFERS_MIN_MB=1`, `CARD_BUFFERS_MB=1024`
  - `VKD3D_CONFIG=descriptor_heap`
- **Windows + AMD, Intel, autres :** valeurs par défaut de VIctory, marquées « non mesuré ».

## Composant par composant

| Composant (avant) | Maintenant sur `rust/full` | Vérification de parité | Ancien code |
| --- | --- | --- | --- |
| `anyxboxx run` (PowerShell) | `anyxboxx run` (`crates/anyxboxx`) | mêmes appels, même environnement `VICTORY_*` et mêmes codes de sortie que le script, avec un faux cargo et un faux victory.exe | **supprimé** ; `tests/test_anyxboxx_run.py` garde les attentes |
| `emu/loader.py` (chargeur PE) | `crates/anyxboxx-pe` (sur `victory-pe`) | `tests/test_rust_pe_parity.py` : 13 images, même JSON et mêmes protections de pages | gardé (le bac à sable Python s'en sert encore) |
| `--rapport` (`emu/report.py`, `config.py`) | `anyxboxx rapport` (`crates/anyxboxx-report`) | `tests/test_rust_rapport_parity.py` : même texte sur 7 cibles, mêmes refus sur 5 | gardé (utilisé par la CLI Python et les outils) |
| `--traduction` | `anyxboxx traduction` | `tests/test_rust_traduction_parity.py` : même tableau, SVG identique à l'octet sur 6 configurations | gardé |
| `--carte` | `anyxboxx carte` | `tests/test_rust_carte_parity.py` : SVG et badges identiques à l'octet sur 5 cas, plus 3 tables GDK synthétiques | gardé |
| `tools/jour_j.py` | `anyxboxx jour-j` | `tests/test_rust_jour_j_parity.py` : partie statique de `rapport.md` identique à l'octet sur 5 cas | gardé : les sections de lancement Unicorn et natif sont déclarées indisponibles en Rust (D2, D4), le portage est partiel |
| `--banc` | `anyxboxx banc` | `tests/test_rust_banc_parity.py` : colonnes Module, Imports, Résolus et pied de tableau | gardé : les colonnes chargement/initialisation demandent le bac à sable Unicorn (D2) et affichent `n/d` |
| `rust/` (textures, son, passerelle RDNA 2) | `crates/` (un seul espace) | tests existants inchangés | `rust/Cargo.toml` supprimé |
| `runtime/xcrdapi` | `crates/anyxboxx-xcrdapi` | `test_xcrdapi.py`, table d'exports identique | **supprimé** |
| `runtime/kernelx` | `crates/anyxboxx-kernelx` (2 661 redirections) | `test_kernelx.py`, table d'exports identique | **supprimé** (la table de redirections est passée dans la crate) |
| `runtime/d3d12_xs` | `crates/anyxboxx-d3d12-xs` | `test_d3d12_virtual_memory.py`, `test_runtime_cpp.py`, table d'exports identique | **supprimé** |
| `runtime/tls` (`anyxboxx_tls.dll`) | `crates/anyxboxx-tls`, cdylib `no_std` | `test_native_tls.py`, `test_native_tls_callbacks.py`, table d'exports identique | **supprimé** |
| `runtime/xmem` | `crates/anyxboxx-xmem` (attributs ERA partagés avec le module xmem de VIctory) | `test_xmem_era.py`, `test_xmem_virtual.py`, `test_runtime_cpp.py`, table d'exports identique | **supprimé** |
| `runtime/xgameruntime` | `crates/anyxboxx-xgameruntime` et `xasync` de VIctory | 17 exports PE et 26 couples fournisseur/interface vérifiés contre le C++ | DLL Rust dans le runtime natif. Inventaire et commandes ci-dessous. |
| `runtime/launcher` | `crates/anyxboxx-native-launcher` | `tests/test_rust_native_launcher_parity.py` lance la suite native synthétique contre les deux implémentations | gardé sous `anyxboxx_cpp.exe` comme oracle ; les cas TomBox demandent ses DLL |
| Lanceur graphique | `crates/anyxboxx-launcher` (nouveau) | 190 tests Rust | |

Comment les DLL Rust et le lanceur natif arrivent dans le dossier du runtime : `cargo build --release` les compile, puis `runtime/CMakeLists.txt` les copie de `target/release` vers `runtime/build/bin` (`rust_dlls.cmake` ; s'il en manque un, la compilation s'arrête en donnant la commande à lancer). Le lanceur natif est `runtime/build/bin/anyxboxx.exe` ; `target/release/anyxboxx.exe` est la CLI VIctory distincte. `cargo build --release` passe donc avant `cmake` (CI, AGENTS.md, README).

Constat sur la porteuse TLS : en cdylib `no_std`, son bloc TLS contient exactement la zone porteuse (262 208 octets, sans variables de thread de la std Rust). Le modèle du jeu la partage donc comme avec la DLL C++. `no_std` exige `panic = "abort"`, que Cargo n'accepte que pour tout le profil release : le profil release de l'espace racine interrompt donc le programme sur panique.

### Inventaire de xgameruntime sous Windows x64

Le lanceur natif charge `runtime/build/bin/xgameruntime.dll`, identique à la DLL Cargo. La cible `xgameruntime_cpp` produit l'oracle dans `runtime/build/oracle/xgameruntime.dll`. Les deux tables PE mesurées correspondent, ordinal et nom compris. Aucun export n’est redirigé.

| Ordinal | Export |
| ---: | --- |
| 1 | `AnyXboxX_CreateDXGIFactory` |
| 2 | `AnyXboxX_CreateDXGIFactory1` |
| 3 | `AnyXboxX_CreateDXGIFactory2` |
| 4 | `AnyXboxX_NormalizeHdrLuminance` |
| 5 | `AnyXboxX_RegCloseKey` |
| 6 | `AnyXboxX_RegOpenKeyExW` |
| 7 | `AnyXboxX_RegOpenKeyW` |
| 8 | `AnyXboxX_RegQueryValueExW` |
| 9 | `InitializeApiImpl` |
| 10 | `InitializeApiImplEx` |
| 11 | `InitializeApiImplEx2` |
| 12 | `QueryApiImpl` |
| 13 | `UninitializeApiImpl` |
| 14 | `XErrorReport` |
| 15 | `XGameRuntimeInitialize` |
| 16 | `XGameRuntimeUninitialize` |
| 17 | `XSystemGetDeviceType` |

La table GDK contient 396 noms de fonctions. 262 appartiennent aux 26 couples que `QueryApiImpl` sert en mode local. Les alias partagent 19 objets. Le tableau donne les 210 emplacements reliés à une fonction spécifique dans les tables C++ après les trois méthodes IUnknown. Certains gestionnaires renvoient une erreur. Les autres emplacements de ces tables passent par le refus générique. Les deux fichiers `.inc` donnent les GUID et les noms de chaque méthode. Sur les 262 fonctions rattachées à ces objets, 52 passent par le refus générique et 12 ont leur propre gestionnaire de refus hors ligne ou de désinstallation. Les rapports comptent les 198 autres (198/396 au total).

| Objet (sans le suffixe `Interface`) | Emplacements C++ |
| --- | --- |
| TaskQueue | 3–7, 9–30 |
| User | 3–14, 16–38, 42–49 |
| Gamertag, Analytics, Feature | 3 |
| System | 3–8 |
| Game, Display, Error | 3–5 |
| Invite, Activation, Protocol | 3–4 |
| PendingInvite | 5–6 |
| HdrDisplay | 4 |
| Networking | 15–17 |
| GameSave | 3–32 |
| LocalStorage | 3–8 |
| Package | 3–19, 22, 24–26, 30, 32–34, 37–41 |
| Store | 3–16, 19–21, 28–29, 49–54, 58–64, 66–67, 71–78, 82–83 |

`test_rust_xgameruntime_parity.py` compare directement les deux DLL dans des processus distincts. Il appelle les initialiseurs et les trois fabriques DXGI, puis vérifie les alias, les réponses IUnknown, les emplacements de refus, les quatre images de profil, les octets d’un gamertag accentué et les callbacks d’un événement Windows. La file Rust gère maintenant les attentes aux emplacements 20 et 21, leur réarmement et leur retrait. Elle conserve les attentes d’une autre file qui partage le port. L’emplacement 0 répond à IUnknown et à l’IID de la file. L’emplacement 29 trace un appel bloquant sur un thread marqué sensible au temps.

`test_rust_dlls.py` exécute les mêmes scénarios sur chaque DLL, dont le démarrage GDK d’un PE synthétique, le registre, les tâches, l’utilisateur, les sauvegardes persistantes, les paquets, les événements, le Store hors ligne, le HDR, la connectivité et la redirection DXGI. Pour XPackage, le C++ renvoie `S_OK` pour la désinstallation de chunks sans rien désinstaller. Rust journalise `E_NOTIMPL`. Les deux renvoient `E_NOTIMPL` pour UWP et `false` pour un paquet. Rust journalise les trois sans activer la trace. Les méthodes Xbox Live et Store non disponibles hors ligne restent limitées comme dans l’oracle.

La file Rust reste l’implémentation `xasync` de VIctory. Les tests couvrent les scénarios nommés ici. L’ordonnancement complet de Microsoft Task, notamment l’ordre et le thread des rappels pendant une terminaison concurrente, reste propre à chaque implémentation. Le lecteur de configuration Rust accepte les attributs sur plusieurs lignes et conserve son contrôle de débordement des composantes de version.

**Compilation et conservation des oracles**

Les trois premières commandes de compilation ci-dessous produisent et installent le runtime Rust. La cible `rust_dlls` suffit à la livraison. La commande CMake suivante construit les trois oracles pour les tests. Le module MIT et `runtime/common` restent nécessaires au C++. Rust embarque les tables `gdk_api_table.inc` et `served_interfaces.inc`. Les rapports Python et Rust lisent ces tables et `unsupported.txt`. Un test vérifie cet inventaire contre les deux DLL. Seuls les rapports sur un ancien dépôt dépourvu de cet inventaire se replient sur les commentaires C++. Les sources C++ et le module MIT Task servent toujours à compiler l’oracle. Les seules copies nécessaires dans la distribution sont les binaires Rust de `bin`. Les dossiers C++ `launcher` et `gpu_runner` fournissent les exécutables de comparaison des tests du lanceur et du GPU.

Signatures vérifiées hors ligne dans les en-têtes du GDK 261000 installés localement, dont `XTaskQueue.h`, `XThread.h`, `XUser.h` et `XPackage.h`. [Référence publique Microsoft GDK](https://learn.microsoft.com/gaming/gdk/)

```powershell
$env:CARGO_BUILD_JOBS = "4"
$env:CARGO_TARGET_DIR = Join-Path $PWD "target-codex"
cargo build --offline --release --workspace
cmake -S runtime -B runtime/build -A x64 "-DANYXBOXX_RUST_RELEASE=$env:CARGO_TARGET_DIR/release"
cmake --build runtime/build --config Release --target rust_dlls
cmake --build runtime/build --config Release --target xgameruntime_cpp anyxboxx_cpp anyxboxx_gpu_run_cpp
$env:ANYXBOXX_REQUIRE_RUNTIME = "1"
python -m pytest -q tests/test_rust_dlls.py tests/test_rust_xgameruntime_parity.py
```

**Encore en Python :**

- Le bac à sable Unicorn et ses fonctions HLE (abandonnés dans l'état final, D2).
- `emu/modules.py`, `emu/loader.py`, `report.py`, `status_map.py`, `translation.py`, `bench.py`, `tools/jour_j.py` : ils ont un portage Rust avec parité, mais d'autres parties Python (la CLI, la fenêtre, le bac à sable) les importent encore. Ils partiront avec le bac à sable.
- RDNA 2 (`emu/rdna2*.py`, phase 4). L'interpréteur de référence est en Rust, crate `anyxboxx-rdna2-ref` (décodeur, table de mnémoniques générée par `tools/rdna2_ref_tables.py`, état du wave, instructions scalaires, branchements, VOP1, VOP2, VOPC, et VOP3 avec sa table native, les encodages plus anciens, retenues, lignes, multiplication 64 bits, SAD, division, comparaisons 64 bits, opérations f64, movrel ; mémoire scalaire (SMEM : chargements, écritures, atomiques) sur une mémoire globale éparse, avec la directive `mem` et la sortie des pages ; SDWA et DPP sur VOP1 et VOP2 ; produits scalaires VOP3P entiers, f16 packés, `v_fma_mix_*`, `v_dot2_f32_f16` et `v_pk_fmac_f16` en VOP2 et VOP3). Aussi en Rust depuis : la LDS (lectures et écritures DS, atomiques, moitiés `_d16`, échanges entre lignes), FLAT, GLOBAL et SCRATCH, les accès MUBUF non typés, les formats typés des tampons MUBUF et MTBUF (`buffer_*_format`, `tbuffer_*_format`, avec la table des formats `format_table.rs` générée par le même outil depuis `emu/data/gfx10_formats.json`), les instructions MIMG 2D (image_load, image_store, image_get_resinfo, image_sample, image_sample_c et image_gather4 avec leurs formes _lz, _l et _o, en refusant ce que le Python refuse), les atomiques flottantes (fcmpswap, fmin, fmax et leurs formes `_x2` sur FLAT et GLOBAL ; fadd, fmin, fmax et fcmpst en f32 et f64 sur la LDS ; `atomic_float.rs`), et SDWA sur les comparaisons VOPC (`ext_compare.rs`). Deux règles des atomiques flottantes sont des hypothèses non vérifiées reprises du Python : fmin et fmax gardent l'autre valeur quand une seule est un NaN (tiré de la description de V_MIN/V_MAX, non mesuré), et le NaN que produit fadd est celui que donne l'arithmétique x86-64 du Python (le premier opérande NaN, l'ancienne valeur d'abord, rendu calme ; le NaN par défaut, signe à 1, pour deux infinis de signes opposés), une propriété de la machine du modèle plutôt qu'un résultat RDNA 2 documenté. Le modèle Python refuse lui-même, et le Rust refuse de même, avec un cas de parité pour chacun : les opérations VOP3P autres que les produits scalaires entiers, f16 packés et formes mixtes ou f16 (les produits scalaires entiers avec op_sel, neg ou clamp ; `v_fma_mix_*` avec op_sel sur une source f32 ; `v_dot2_f32_f16` avec op_sel[2] ou neg_hi[2] ; `v_pk_fmac_f16` en SDWA ou DPP, et avec des modificateurs VOP3 autres que clamp), les formes op_sel et clamp de la seule opération VOP3 à résultat 16 bits (`v_sat_pk_u8_i16` ; op_sel n'est pas lu sur les résultats 32 bits), DPP16 et DPP8 sur VOPC, et les formes `_d16` des accès typés aux tampons et des images. Aussi en Rust depuis : les instructions graphiques, les exports EXP (`export.rs`) et l'interpolation VINTRP (`interp.rs` ; la ligne de commande prend `primitive N` et `primitives L:P ...` et écrit des lignes `exp CIBLE LIGNE X Y Z W`), les instructions du compteur de programme (s_getpc_b64, s_setpc_b64, s_swappc_b64, s_call_b64 ; `pc.rs`) et les contrôles de cache MUBUF (buffer_gl0_inv, buffer_gl1_inv). Aucune famille d'instructions RDNA 2 de l'interpréteur n'est plus seulement en Python : un balayage de chaque encodage que le Python exécute (1 060 des 1 490 encodages de LLVM plus les cinq propres à l'ISA, opérandes à zéro, wave32 et wave64, lancé le 2026-11-25) n'en trouve aucun que le Python exécute et que le Rust refuse. Le Rust refuse aussi ce que le Python y refuse : EXP avec COMPR (exports 16 bits), les attributs VINTRP au-delà de param7 et l'opcode VINTRP 3, et les formes VOP3 de `v_interp_*`. Les NaN que donnent v_interp_p1_f32 et v_interp_p2_f32 sont, comme ceux de fadd, ceux de l'arithmétique x86-64 du Python (l'opérande NaN de gauche d'abord, rendu calme ; le NaN par défaut, signe à 1, pour ∞ × 0 ou ∞ − ∞) : une propriété de la machine du modèle, gardée comme hypothèse non vérifiée. Les règles des formats typés (arrondi UNORM et SNORM, traitement de DST_SEL, FORMAT INVALID du V# qui désactive MTBUF, restrictions en écriture) suivent le modèle Python : ce que dit l'ISA RDNA 2 d'AMD (section 8.1.4, table 31, p. 59 ; section 8.1.5, p. 62) y est cité, et le reste (arrondi au plus proche en UNORM/SNORM, élargissement des NaN FLOAT 16 bits, saturation des écritures UINT/SINT, 1.0 contre 1 pour DST_SEL = 1) est marqué supposé, non mesuré, jusqu'à ce que les tests de LLVM ou notre propre mesure le tranchent. La division utilise `num-bigint` (Rust pur, crates.io) pour l'arrondi exact. Parité avec le modèle Python : `tests/test_rdna2_ref_parity.py` (programmes fixes, 300 programmes aléatoires à graine fixe et 200 pour les f16 packés et `v_pk_fmac_f16`, comparés ligne à ligne). D'ici là, `anyxboxx traduction` lit la couverture RDNA 2 dans `crates/anyxboxx-report/data/rdna2-coverage.txt` ; un test échoue quand cette table s'écarte de la mesure Python, et `python tests/test_rust_traduction_parity.py` la régénère.
- `tools/*.py` et la suite pytest (l'oracle).

## Vérifications sur cette branche (25/11/2026, Windows 11, Rust 1.98.1, Python 3.12)

```
python -m pytest                       → 1251 passed, 11 skipped
python -m ruff check --select W605,F,E9 PrototypeScarlett tests tools → All checks passed!
cargo test --release --workspace       → 198 passed, 0 failed
cargo clippy --release --workspace --all-targets → 0 warnings
cargo fmt --check                      → propre
cargo test --release -p anyxboxx-xgameruntime → 65 réussis (dont la régression de course XAsync entre nettoyage et état)
cargo test -p anyxboxx-rdna2-ref      → 122 passed (décodeur, wave, mémoire, atomiques entières et flottantes, scalaires, vectoriel, SDWA/DPP, SDWA sur VOPC, comparaisons, VOP3, SAD, division, f64, movrel, produits scalaires, LDS, mémoire vectorielle, formats typés, images, exports, interpolation, compteur de programme, contrôles de cache, boucle d'exécution)
python -m pytest tests/test_rdna2_ref_parity.py → 38 passed sur 38 (f16 packés et `v_pk_fmac_f16` : 200 programmes à graine fixe, programmes fixes, EXP (120 programmes à graine fixe), VINTRP (150 programmes à graine fixe), les instructions du compteur de programme, les contrôles de cache MUBUF, 300 aléatoires, tests dédiés à la division et aux f64, mémoire scalaire, LDS, FLAT/GLOBAL/SCRATCH, MUBUF, formats typés et images, atomiques flottantes, SDWA et DPP sur VOPC (200 programmes à graine fixe), et les refus communs aux deux modèles : `_d16` typés et images, VOP3P au-delà des produits scalaires, op_sel 16 bits de VOP3 ; modèle Python contre binaire Rust). `test_wide_operations_match_the_python_model` passe depuis `ea9fee97` (NaN canonique pour sqrt et rsq f64 d'entrées négatives).
```

Le 26-11-2026, `cargo build --release --workspace`, `cargo test --release --workspace`, Clippy sur la crate avec `-D warnings`, `cargo fmt --check` et Ruff ont réussi. CMake a compilé `runtime/build-ninja` avec Ninja. Les 89 tests pytest utilisent la DLL xgameruntime Rust dans un runtime mixte ; `tests/test_rust_dlls.py` compare aussi ses exports à la DLL C++. Aucun jeu n’a été lancé.

Le 27 novembre 2026, les 566 tests Cargo du workspace en release et les 1 969 tests pytest ont réussi avec le runtime obligatoire, les trois oracles C++ compilés et WARP pour les tests GPU. Les tests du lanceur ont utilisé leur enregistreur synthétique sous le nom `axg-recorder.exe`. Aucun jeu ni exécutable VIctory n’a été lancé. Le contrôle Ruff documenté, les 17 tests unitaires xasync de VIctory et `cargo xtask policy` ont également réussi.

Le runtime a été compilé depuis cette branche (cargo, puis CMake). Avec `ANYXBOXX_REQUIRE_RUNTIME=1`, les tests des DLL de TomBox demandent aussi `python tools/build_tombox.py`.

## Constats en chemin

- À ce commit, VIctory ne compile pas sans sa fonctionnalité `diagnostics` (`victory_gpu::queue::presented` en dépend mais sert hors d'elle). L'espace racine dépend de `victory-hle` avec cette fonctionnalité, comme VIctory est toujours compilé.
- VIctory n'écrit les i/s que dans le titre de sa fenêtre. Une ligne d'i/s sur stdout dans VIctory permettrait aux deux lanceurs de l'afficher.
- Le test `a_read_in_place_never_meets_the_gap_of_a_remapping` de VIctory (victory-hle, xmem) est instable : il échoue environ 3 fois sur 8 sur le fichier non modifié.
- Le profil Windows+NVIDIA a été corrigé après ton essai : les interrupteurs de la PR #199 à `0` n'ont rien changé (2-3 i/s sur `main`, 5-7 i/s sur le fork à `80d58f8`).
- Bogue du portage Rust, corrigé : cinq endroits de `crates/anyxboxx-rdna2-ref` indexaient un groupe de VGPR sans vérifier qu'il finit à v255, et le binaire s'arrêtait sur un indice hors bornes (`wide.rs`, `div.rs`, `vop3_forms.rs`, deux fois dans `sad.rs`) : un résultat 64 bits écrit en v255 par les opérations f64 et les conversions vers f64, les décalages 64 bits, `v_div_*_f64`, `v_mad_u64_u32` et les formes SAD empaquetées, la paire S2 de 64 bits des formes SAD empaquetées en v255, et la source et le résultat 128 bits de `v_mqsad_u32_u8` à partir de v253 (29 mnémoniques, trouvés en essayant chaque opcode VOP1, VOP2, VOP3, VOP3P et VOPC avec des registres en haut de la plage ; Linux, 2026-11-26). Ils rendent maintenant l'erreur d'opérande que `read64` rendait déjà pour une paire source en v255, avant toute écriture (`Wave::vgpr_group`). Le modèle Python lève `IndexError` dans les mêmes cas, après avoir écrit la moitié basse dans la première ligne active ; savoir s'il doit plutôt lever `Untranslated`, et le Rust suivre, reste ouvert.
- CI sur `rust/full` (première exécution via les déclencheurs de la PR #228) : le travail pytest échouait à la copie des DLL Rust, car `file(COPY_FILE)` ne crée pas `runtime/build/bin` sur une compilation propre. Corrigé dans `7bc532ba` (`file(MAKE_DIRECTORY)` d'abord). Une exécution `tests.yml` lancée à la main sur `rust/full` passe ensuite dans les trois travaux.
- Bogue du portage Rust, corrigé : `crates/anyxboxx-rdna2-ref/src/scalar.rs` étendait par des zéros un littéral de 32 bits lu comme S0 par `s_ashr_i64`, `s_bfe_i64` et `s_flbit_i32_i64`, là où le modèle Python l'étend par le signe (`signed_literal_s0` dans `emu/rdna2_exec_bits.py`, d'après l'ISA RDNA 2 d'AMD, section 6.2.1, p. 43 : « 64-bit signed integer: sign extended to 64 bits »). Avec le littéral 0xFFFFFFF0, `s_flbit_i32_i64` rendait 32 au lieu de 60. Le Rust étend maintenant par le signe pour ces trois instructions ; les sources 64 bits non signées et bit à bit restent étendues par des zéros. Trouvé en lançant le générateur de parité sur 48 000 graines de plus (Linux, 2026-11-26) : les 300 programmes à graine fixe n'associaient aucune de ces trois instructions à un littéral négatif.
- Le test xmem `graphics_blocks_get_their_own_region_flagged_by_the_query` échouait de temps en temps : les tests partagent un seul espace d'adressage, et une région libérée par un test était reprise par un autre qui tournait en même temps. Les tests xmem passent maintenant à tour de rôle sur un même verrou (`7b69e6fb`) ; 15 exécutions de suite passent.
- Bogue du portage Rust, corrigé : `crates/anyxboxx-rdna2-ref/src/movrel.rs` additionnait le numéro de registre et M0 sur 32 bits. M0 est une valeur de 32 bits entière : avec M0 = 0xFFFFFFFD, `s_movrels_b32 s102, s47` lisait s44 en compilation release (la somme rebouclait) et paniquait en compilation debug (`attempt to add with overflow`), là où le modèle Python refuse l'indice 0x10000002C, hors du modèle. La somme est maintenant faite sur 64 bits et refusée comme en Python. Même chose pour `s_movrels_b64`, `s_movreld_b32/b64`, `v_movrels_b32`, `v_movreld_b32` et `v_movrelsd_b32` ; les formes `_2` et `v_swaprel_b32` prennent des champs de 10 bits de M0 et ne pouvaient pas déborder. Trouvé en lançant le générateur de parité sur 6 000 graines de plus, avec des valeurs spéciales dans les registres scalaires (Linux, 2026-11-26) ; les 300 programmes à graine fixe gardent M0 sous 256 tant qu'aucune instruction ne le réécrit.
- Le générateur de parité RDNA 2 envoyait parfois les branchements dans des mots de données, décodés en instructions mémoire (hors du modèle). Les branchements tombent désormais sur des débuts d'instruction.
- Bogue du modèle Python (non corrigé ici, à signaler dans une issue) : les formes VOP3 de `v_madmk_f32`, `v_madak_f32`, `v_fmamk_f32` et `v_fmaak_f32` (opcodes VOP3 0x120, 0x121, 0x12C, 0x12D) sans opérande littéral lèvent `TypeError` dans `emu/rdna2_exec_vop3.py` (la constante K absente arrive à l'opération comme `None`) au lieu d'être refusées ou exécutées. Le Rust lit K comme 0. Aucun des deux résultats n'est un refus, les tests de parité laissent donc ces formes de côté ; trouvé par le balayage des encodages ci-dessus.
- Bogue du modèle Python (non corrigé ici, à signaler dans une issue) : `emu/rdna2_exec_wide.py`, `_fma`, calcule `float(Fraction(a) * Fraction(b) + Fraction(c))` pour des opérandes finis, et `float()` lève `OverflowError` quand le résultat exact dépasse l'intervalle des doubles, au lieu de rendre ±infini comme le fma IEEE. Le portage Rust rend ±infini ; le test aléatoire général laisse donc `v_fma_f64` de côté et un test dédié le couvre avec des opérandes qui ne peuvent pas déborder.
- Bogue du portage Rust, corrigé : `v_mac_f32`, `v_madmk_f32` et `v_madak_f32` dans `crates/anyxboxx-rdna2-ref/src/vector.rs` calculaient `a * b + c` comme une simple expression, si bien que le compilateur choisissait quel NaN sortait, et il inversait les opérandes de l'addition : avec un facteur NaN et un terme ajouté NaN, le Rust rendait le NaN du terme ajouté là où le modèle Python (`_mul_add`, arithmétique x86-64) rend celui du facteur (par exemple `v_madak_f32` avec S1 = `0x7FC00B0B` et K = `0x7FC00C0C` : Rust `0x7FC00C0C`, Python `0x7FC00B0B`). `float::mul_add` écrit maintenant l'ordre du modèle : le premier facteur NaN, ou le NaN que fait le produit (∞ × 0), avant un terme ajouté NaN. `v_mad_f32` l'utilise aussi ; il rendait le NaN du modèle dans cette compilation, mais seulement par le choix du compilateur. Comme les autres règles de NaN du portage, cela suit l'arithmétique de l'hôte du modèle, pas un résultat mesuré sur RDNA 2. Trouvé en lançant le générateur de parité sur 60 000 graines de plus (Linux, 2026-11-27) : cinq programmes finissaient avec un NaN différent dans un registre, les trois examinés tous sur un `v_madak_f32` avec un K NaN ; avec ce changement, les mêmes 60 000 programmes finissent avec les mêmes valeurs dans les deux modèles.
- Incohérence du modèle Python, corrigée : la forme VOP3 de `v_cndmask_b32` lisait son masque de lignes (S2) dans la boucle sur les lignes actives (`emu/rdna2_exec_vop3.py`) ; avec EXEC = 0, un opérande de masque que le modèle refuse (un VGPR, un code réservé) n'était donc jamais lu et l'instruction s'exécutait, alors que le portage Rust lit le masque une fois et la refusait. Le Python le lit maintenant une fois avant les lignes, comme ses autres contrôles VOP3 et comme le Rust ; rien ne change pour un masque lisible. Trouvé par le générateur de parité : sur 120 000 graines de plus le 27/11/2026, c'était la seule différence restante entre les deux modèles en dehors des groupes de registres au-delà de v255 (#437). (Un balayage de tous les opcodes montre aussi les instructions en demi-précision encore en cours de portage, #503 et #505 ; le générateur ne les tire pas encore.)
