# EasyUV

Dépliage UV automatique intelligent, écrit en Rust : **application Windows**, **outil en ligne de commande** et **addon Blender** qui partagent le même moteur.

- Coutures placées automatiquement, dépliage à faible distorsion (LSCM + ARAP).
- Densité de texels uniforme et contrôlable en px/m.
- Damier de contrôle.
- Interface inspirée de Blender.
- Mises à jour automatiques à chaque release.

## Télécharger

Page des [releases](https://github.com/lburdier/EasyUV/releases/latest) :

| Fichier | Contenu |
|---|---|
| `EasyUV-windows-x64.zip` | Application + `easyuv-cli.exe` |
| `easyuv-blender-<version>.zip` | Extension Blender 4.2+ / 5.x |

### Mises à jour automatiques

- **Application :** au démarrage, EasyUV interroge la dernière release GitHub. Si une version plus récente existe, un bouton orange « Mise à jour disponible » apparaît. L'installation télécharge le nouvel exécutable, **vérifie son empreinte SHA-256**, le remplace et relance l'application. Le menu *Aide > Vérifier les mises à jour* fait la même chose à la demande. Pour désactiver la vérification au démarrage, définissez `EASYUV_NO_UPDATE=1`.
- **Blender :** *Préférences > Get Extensions > Repositories > `+` > Add Remote Repository*, URL :
  `https://github.com/lburdier/EasyUV/releases/latest/download/index.json`
  Cochez *Check for Updates on Startup* : Blender proposera chaque nouvelle version.

## Comment le dépliage décide

1. **Coutures de départ :** arêtes vives (angle dièdre réglable), coutures déjà marquées, géométrie non-manifold.
2. **Topologie :** chaque îlot doit être un disque.
   - tube (bras, cylindre) → une seule couture, le long du chemin le plus discret entre ses bords, de préférence dans un pli ;
   - forme fermée ou trouée (sphère, tore) → partition géodésique dont la frontière suit les plis.
3. **Dépliage :** LSCM (conforme), puis relaxation **ARAP** (préserve les aires).
4. **Contrôle qualité :** un îlot trop déformé ou avec des triangles retournés est redécoupé, récursivement.
5. **Densité de texels :** les îlots sont à l'échelle réelle (espace monde, mètres), donc la densité est la même partout. Deux modes : *Remplir 0-1* ou *Densité cible*.
6. **Plusieurs objets :** rangement dans une tuile commune (même densité pour tous) ou une tuile par objet.
7. **Empaquetage :** rotation qui minimise la boîte englobante, puis skyline avec rotation de 90°.

## Auto-apprentissage

EasyUV s'améliore à force de déplier.

1. **Empreinte.** Chaque maillage est décrit par une empreinte invariante d'échelle : taille, part de triangles, arêtes vives, courbure, bords ouverts, topologie, forme.
2. **Prédiction.** Un modèle de substitution à noyau estime, à partir des expériences sur des maillages similaires, le gain de chaque réglage (angle de couture, tolérance de distorsion, ARAP) par rapport aux réglages d'usine.
3. **Essais.** EasyUV essaie en parallèle la prédiction, les réglages d'usine, vos réglages et quelques variantes, puis garde le meilleur. L'exploration diminue quand la confiance augmente.
4. **Mémoire.** Les mesures de chaque essai (distorsion, coutures, îlots, remplissage, retournements) sont enregistrées. Elles servent pour tous les maillages qui suivent.

Le curseur **Priorité : coutures ↔ distorsion** change le compromis immédiatement, sans réentraînement : les mesures sont stockées brutes et le score est recalculé.

Le modèle se trouve dans `%APPDATA%\EasyUV\apprentissage.txt`. Il est partagé par l'application, le CLI et Blender, et écrit en ajout seul. Un modèle de base pré-entraîné est livré. Pour l'enrichir d'un coup sur vos propres modèles :

```text
easyuv-cli --train D:\mes_modeles --rounds 3      # entraîner
easyuv-cli --eval D:\autres_modeles               # mesurer le gain sur des maillages jamais vus
```

## Formats

| Format | Import | Export |
|---|---|---|
| OBJ (multi-objets) | oui | oui |
| glTF / GLB | oui (quads reconstruits) | GLB |
| FBX | oui (natif, via [ufbx](https://github.com/ufbx/ufbx)) | oui, via Blender installé |

Les transformations des objets sont appliquées (espace monde). Le FBX et le glTF sont convertis en Z vers le haut et en mètres.

## Application

Raccourcis :

| Touche | Action |
|---|---|
| `U` | Déplier |
| `F` / `Origine` | Cadrer la sélection / tout (dans la vue sous le pointeur) |
| Molette / clic + glisser | Orbite |
| Maj + glisser / clic droit | Déplacer la vue |
| Molette | Zoom |
| `Ctrl+O` / `Ctrl+I` / `Ctrl+S` | Ouvrir / importer dans la scène / exporter |

Glisser-déposer un fichier pour l'ouvrir, avec `Maj` pour l'ajouter à la scène. Modes d'affichage : Solide, Damier, Îlots, Étirement.

### Outils UV

Panneau « Outils UV », menu UV et clic droit dans l'éditeur UV, dans l'esprit du UV Toolkit de Maya et de RizomUV.

Modes de sélection, comme Blender : sommets `1`, arêtes `2` (double-clic : boucle), faces `3`, îlots `4`. Avec une sélection, `U` ne déplie qu'elle ; `Maj+U` déplie les objets.

| Outil (touche dans l'éditeur UV) | Effet |
|---|---|
| Sélection `Q` | clic, `Maj`+clic, rectangle ; glisser, `G` `R` `S` pour transformer |
| Pinceau Déplacer `Maj+1` | tire les UV sous le pinceau, l'effet s'atténue vers le bord |
| Pinceau Détendre `Maj+2` | les arêtes reprennent leur longueur 3D (moins d'étirement) |
| Pinceau Redresser `Maj+3` | les arêtes s'alignent sur l'horizontale ou la verticale la plus proche : rend droite la zone peinte |
| Pinceau Pincer `Maj+4` | resserre les UV vers le centre, `Maj` pour les écarter |
| Rectangle `Maj+5` | dessinez un rectangle : l'îlot sélectionné y est rendu droit puis étiré pour le remplir |

Coutures (sur les arêtes sélectionnées) : Couper, Coudre (rapprocher et souder les îlots), Marquer la couture pour les prochains dépliages, Retirer la marque.

Sélection par zone : rectangle, lasso ou cercle (`W`), `Maj` pour ajouter, `Ctrl` pour retirer. Sélection par angle : arêtes vives (mode arêtes) ou faces d'un même pan.

Vue 3D comme Blender : gizmo cliquable et pavé numérique pour les vues d'axe orthographiques ; `Tab` passe les objets sélectionnés en mode Édition, où l'on choisit leurs sommets, arêtes et faces sur le modèle (`Alt`+clic : boucle, `L` / `Ctrl+L` : relié, `Ctrl+E` : couture).

Projeter depuis la vue : les UV prennent ce que l'on voit dans la vue 3D (comme Project From View de Blender), rangées dans la place libre ou étirées sur la tuile (Cadré).

Grille parfaite (`Maj+G`) : rangées et colonnes sur des droites. Remettre d'aplomb : îlots tournés d'après leurs arêtes.

Rendre droit (`Maj+W`) : grille de quads, arc déroulé ou rectangle à bord épinglé, la méthode la moins déformée l'emporte. En mode sommets, 4 sommets choisis sur le bord d'une bande en sont les coins imposés.

Damiers : Couleur, Mono, Numéroté, Cercles, Flèches, Dégradé UV, Diagonales. Les îlots sélectionnés et l'îlot survolé dans l'éditeur UV sont mis en évidence sur le modèle 3D.

Le score UV (sur 100) est affiché en direct dans l'en-tête de l'éditeur UV : distorsion, chevauchements, faces retournées, remplissage, densité égale, coutures.

Rayon du pinceau : `[` `]` ou `Ctrl`+molette. `Échap` annule le coup de pinceau, `Ctrl+Z` après.

**Zone du pinceau.** Les pinceaux ne touchent que la sélection : un îlot (clic), des faces ou des sommets. Le reste est figé et estompé à l'écran. Sans sélection, tout ce qui est visible peut bouger. Les sommets partagés avec une face hors de la zone restent figés, donc les faces voisines ne se déforment pas.

**Isoler une zone.** `H` (ou *UV > Isoler la sélection*) fige tout sauf la sélection. La zone reste isolée quand vous changez de sélection, et `G` / `R` / `S` n'en sortent pas non plus. `Alt+H` la libère.

Outils d'îlots (sur la sélection) : aligner (gauche, centre, droite, haut, bas), répartir en U ou en V, pivoter d'un angle, miroir, échelle, empiler, égaliser la densité, remplir la tuile, rendre droit, orienter, optimiser (ARAP), redéplier.

## Ligne de commande

```text
easyuv-cli modele.fbx                          # -> modele_uv.obj
easyuv-cli a.glb b.obj -o scene.glb            # une scène, rangement commun
easyuv-cli dossier\ --format fbx --separate    # traitement par lots
easyuv-cli x.obj --density 512 --tex 2048      # densité cible 512 px/m
easyuv-cli --check-update | --self-update
```

## Blender

Panneau : Vue 3D → barre latérale `N` → onglet **EasyUV** (aussi dans l'éditeur UV). Raccourci `Alt+U` en mode édition, et menu `U` → *Dépliage intelligent*.

Fonctions :
- dépliage de la sélection, ensemble ou objet par objet ;
- respect des coutures et arêtes vives existantes ;
- marquage des coutures calculées ;
- mesure, copie et application de la densité ;
- damier activable et désactivable, avec restauration des matériaux d'origine.

## Compiler

Prérequis : Rust stable (MSVC), Python 3.11+, et Blender pour les tests de l'addon.

```powershell
.\build.ps1                                   # compile et prépare dist\
cargo test --release --workspace
blender --background --factory-startup --python tests\blender_test.py
blender --background --factory-startup --python tests\make_models.py   # modèles de test
```

| Dossier | Rôle |
|---|---|
| `crates/easyuv-core` | Moteur (aucune dépendance) + API C `easyuv.dll` |
| `crates/easyuv-app` | Application egui, `easyuv-cli`, import/export, mises à jour |
| `blender_addon/easyuv` | Extension Blender (ctypes → `easyuv.dll`) |
| `tools/package.py` | Assets de release + index du dépôt d'extensions Blender |

## Publier une version

1. Mettre la même version dans `Cargo.toml`, `blender_addon/easyuv/blender_manifest.toml` et `bl_info`.
2. Ajouter une section `## [x.y.z]` dans `CHANGELOG.md`.
3. Lancer `git tag vx.y.z && git push origin vx.y.z`.

GitHub Actions teste, compile et publie la release. L'application et Blender la proposent ensuite automatiquement.

## Licence

[GPL-3.0-or-later](LICENSE), compatible avec l'écosystème Blender.
