# Lanceur graphique : installation et utilisation

Version anglaise : [../launcher.md](../launcher.md). Dans `main`. Code : `crates/anyxboxx-launcher`.

`anyxboxx-launcher.exe` est une fenêtre qui vérifie votre machine et votre dossier du jeu, compile VIctory, préchauffe le cache des shaders et lance le jeu avec les réglages mesurés de `anyxboxx run` (les mêmes que `tools\lancer_victory.ps1` dans `main`). Il ne contient ni ne télécharge aucun fichier du jeu, et n'écrit jamais dans le dossier du jeu.

## Ce qu'il faut

- Windows 10 ou 11 x64, un processeur avec AVX2, une carte Direct3D 12 avec le shader model 6.6 et les mesh shaders (NVIDIA RTX 20 ou plus récente, AMD RDNA 2 ou plus récente), 32 Go de RAM conseillés. Liste complète : [lancer-gta6.md](lancer-gta6.md#prérequis).
- Votre propre copie du build Xbox Series 1.0.2.1 : un dossier qui contient `game_scarlett_master.exe`. Non fournie.
- [Git](https://git-scm.com/), [Rust via rustup](https://rustup.rs/), Visual Studio 2022 ou 2026 Build Tools avec la charge « Développement Desktop en C++ ». Le lanceur compile VIctory depuis les sources avec cargo au premier appui sur JOUER : Rust et MSVC restent nécessaires même avec le lanceur précompilé.
- De la place : la compilation de VIctory prend quelques Go et le cache des shaders grossit en jouant. Gardez plus de 30 Go libres sur le disque système (à 10 Go, le chargement ne finit jamais).

## Installation

### 1. Récupérer le code

```powershell
git clone https://github.com/LooperSalty/AnyXboxX.git
cd AnyXboxX
```

VIctory est inclus dans `third_party/victory` : rien d'autre à télécharger.

### 2. Récupérer le lanceur

Au choix :

- **Précompilé** : téléchargez `anyxboxx-launcher.exe` depuis les [releases du lanceur](https://github.com/LooperSalty/AnyXboxX/releases?q=launcher&expanded=true), comparez son SHA-256 à `anyxboxx-launcher.exe.sha256` (`Get-FileHash .\anyxboxx-launcher.exe`), puis placez-le **dans le dossier `AnyXboxX`** cloné (ou un de ses sous-dossiers).
- **Depuis les sources** : `cargo build --release -p anyxboxx-launcher`, puis `target\release\anyxboxx-launcher.exe`.

Le lanceur trouve le dépôt en remontant depuis son propre dossier jusqu'à `third_party\victory\Cargo.toml`. S'il est rangé ailleurs, définissez `ANYXBOXX_REPO` sur le dossier cloné (`setx ANYXBOXX_REPO C:\chemin\vers\AnyXboxX`, puis ouvrez une nouvelle fenêtre). Sans l'un ou l'autre, JOUER s'arrête sur « repository not found ».

### 3. Premier démarrage

1. Double-cliquez sur `anyxboxx-launcher.exe`.
2. Page **Setup** : chaque ligne doit être verte ou expliquée. Rust, le runtime Direct3D 12, le redistribuable VC++ et le pilote graphique sont vérifiés ; `dxc.exe` du Windows SDK ne sert qu'aux tests GPU du dépôt. Les liens mènent uniquement aux pages officielles ; le lanceur n'installe rien lui-même.
3. Page **Game folder** : choisissez le dossier qui contient `game_scarlett_master.exe` (sélecteur ou détection automatique). La vérification rapide ne lit que les fichiers de ce dossier.
4. **Accueil** : appuyez sur **JOUER**. La première fois, un écran plein montre les étapes : compilation de VIctory (plusieurs minutes, une seule fois), préchargement des shaders, puis démarrage du jeu. Annuler arrête l'étape en cours.
5. Le jeu s'ouvre dans sa propre fenêtre et le lanceur se ferme, sauf si « Keep the launcher open » est coché.

Le premier lancement après un nouveau build ou un nouveau réglage recompile des centaines de pipelines et tourne bien en dessous de 1 fps pendant un moment : jugez la vitesse au deuxième lancement, cache rempli.

## Options

- Résolution et mode de fenêtre (fenêtré ou sans bordure).
- Disposition du clavier : automatique, QWERTY, AZERTY ou QWERTZ (`VICTORY_KEYBOARD`).
- Rapide (par défaut, réglages mesurés), Qualité (meilleurs éclairages et textures, moins d'images par seconde), carte 12 Go ou carte 8 Go. Les réserves du profil Rapide sur la carte (1 536 Mo de fenêtres de données de dessin et de copies de tampons) ont été dimensionnées sur des cartes de 12 à 16 Go ; sur une RTX 5060 8 Go, il occupait 8 010 Mo pour un budget de 6 823 Mo, Windows déplaçait le reste dans la mémoire du PC et la file des textures était refusée. Les deux préréglages de carte réduisent ces réserves (1 024 Mo, ou 512 Mo avec une réserve de listes de 256 Mo) et plafonnent les textures du jeu avec `VICTORY_TEXTURE_MAX_SIZE` (2048, ou 1024 texels) : les niveaux d'une texture au-dessus du plafond ne reçoivent pas de mémoire sur la carte et ses vues commencent au premier niveau sous le plafond ; toutes les textures tiennent, un peu moins nettes de près. Le préréglage 12 Go n'est pas encore mesuré sur une carte de 12 Go.
- Compteur d'images, appui automatique sur A pendant les écrans de démarrage, CSV des temps d'image dans le dossier du cache.
- Panneau avancé : surcharge de variables `VICTORY_*` une à une ; elles priment toujours sur le profil.
- NVIDIA Smooth Motion (RTX séries 40 et 50) : la génération d'images du pilote lui-même, qui insère une image générée entre deux images du jeu. Deux fois plus d'images affichées ; la réactivité reste celle des vraies images du jeu. Ce n'est pas une variable de VIctory : à chaque PLAY, le lanceur l'écrit dans le profil du pilote NVIDIA qui contient `victory.exe` (créé s'il n'y en a pas), via NVAPI, avec les identifiants de réglage qu'utilise NVIDIA Profile Inspector (`0xB0D384C0` activation, `0xB0CC0875` API autorisées : Direct3D 12). Aucun autre réglage du pilote n'est touché, et aucun droit administrateur n'est nécessaire.

Les réglages sont des profils par système et par fabricant de carte (`crates/anyxboxx-launcher/profiles.toml`). Seul le jeu Windows + NVIDIA est mesuré ; les autres reprennent les valeurs par défaut de VIctory.

## Fichiers écrits

| Chemin | Contenu |
| --- | --- |
| `%LOCALAPPDATA%\AnyXboxX\launcher\settings.toml` | Dossier du jeu et options |
| `%LOCALAPPDATA%\AnyXboxX\logs\game-*.log` | Le journal de chaque partie (sortie de VIctory) |
| `%LOCALAPPDATA%\AnyXboxX\launcher\splash\` | Facultatif : vos propres images PNG ou JPEG, en diaporama. Rien n'y est fourni. |
| `%LOCALAPPDATA%\victory\data\` | Sauvegardes, données et `crashes.log` de VIctory |
| `third_party\victory\target\release\` | Le build de VIctory |
| Réglages du pilote NVIDIA | Quand Smooth Motion est activé ou désactivé : les réglages Smooth Motion du profil qui contient `victory.exe` (`AnyXboxX (victory.exe)` quand le lanceur le crée) |

## Dépannage

- **« repository not found »** : l'exécutable est hors du dépôt cloné et `ANYXBOXX_REPO` n'est pas défini (étape 2).
- **La compilation de VIctory échoue** : lancez `cargo build --release -p victory --manifest-path third_party\victory\Cargo.toml` dans un terminal pour lire le message de cargo ; il manque le plus souvent la charge MSVC.
- **Un jeu tourne déjà** : le lanceur n'autorise qu'un jeu à la fois, car deux partageraient le dossier des sauvegardes.
- **La fenêtre s'est fermée seule** : le journal indique `window closed: suspending the title` quand c'est vous qui l'avez fermée ; un plantage est écrit dans `crashes.log` sous `%LOCALAPPDATA%\victory\data\`.
- **Erreur Windows 1455 dans `crashes.log`** : le fichier d'échange est trop petit ; fermez d'autres applications ou agrandissez-le.
- **Le lanceur n'ouvre jamais sa fenêtre** (le processus reste dans le Gestionnaire des tâches) : sous Windows, sa fenêtre est dessinée avec Direct3D 12. Pour essayer une autre API graphique, lancez-le depuis une invite de commandes après `set WGPU_BACKEND=vulkan` (ou `dx12`, la valeur par défaut) ; voir l'issue #483.

Sous Proton ou Wine, le lanceur ne peut pas compiler VIctory (pas de cargo) : utilisez `anyxboxx run` comme décrit dans [lancer-gta6.md](lancer-gta6.md).
