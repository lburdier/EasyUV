# Changelog

## [0.3.0] - 2026-10-23

### Pinceaux UV
- Nouveaux outils dans l'éditeur UV (touches `Q`, `1` à `5`, ou panneau « Outils UV ») :
  - **Déplacer** : tire les UV sous le pinceau, avec une atténuation douce vers le bord.
  - **Détendre** : les arêtes reprennent leur longueur 3D, l'étirement disparaît là où l'on peint.
  - **Redresser** : les arêtes s'alignent sur l'horizontale ou la verticale la plus proche ; la zone peinte devient droite.
  - **Pincer** : resserre les UV vers le centre du pinceau (`Maj` : les écarte).
- Cercle d'influence sous le pointeur, rayon avec `[` `]` ou `Ctrl`+molette, force réglable. Le pinceau agit sur les îlots sélectionnés, ou sur tous. `Échap` annule le coup de pinceau, `Ctrl+Z` ensuite.

### Outil Rectangle
- On dessine un rectangle dans l'éditeur UV : l'îlot sélectionné (ou celui sous le rectangle) est rendu droit, tourné pour suivre le côté long du rectangle, puis étiré pour le remplir exactement.

### Outils d'îlots (façon Maya UV Toolkit / RizomUV)
- **Aligner** à gauche, au centre, à droite, en haut, en bas ; **Répartir** en U ou en V à écarts égaux.
- **Pivoter** d'un angle libre, ±90°, 180° ; **Miroir** U ou V ; **Échelle** ×2 ou ×½.
- **Empiler** les îlots les uns sur les autres ; **Égaliser la densité** de texels de la sélection ; **Remplir la tuile**.
- **Optimiser** (relaxation ARAP, comme « Optimize » de RizomUV) et **Redéplier** (LSCM + ARAP) les îlots sélectionnés, en gardant leur place, leur orientation et leur taille.
- Tous ces outils sont aussi dans le menu UV et le clic droit de l'éditeur UV. « Aligner sur les axes » s'appelle désormais « Orienter ».

## [0.2.1] - 2026-10-23

### Interface
- **Séparateur déplaçable** entre la vue 3D et l'éditeur UV : glisser pour agrandir l'une ou l'autre (chaque vue garde au moins 220 px), double-clic pour revenir au partage par défaut. Réglage propre aux espaces Layout et UV Editing.
- Dans une vue 3D étroite, le titre s'efface au profit des boutons d'affichage.

### Rendre droit
- **Bandes courbes redressées proprement** même quand la grille de quads ne couvre pas tout l'îlot (triangles, n-gones, jonctions en T, bout de bande irrégulier) : un arc de cône est déroulé autour de son centre (coordonnées polaires), au lieu de laisser le bout écrasé ou replié.
- Chaque résultat est contrôlé (faces retournées ou écrasées) et le plus propre est retenu. Si le redressement devait abîmer l'îlot (couvercle, disque entouré d'anneaux), l'îlot reste tel quel et la barre d'état le signale.

## [0.2.0] - 2026-10-22

### Objets tournés
- **Objets tournés détectés en Auto** (pots, couvercles, bouteilles, tuyaux biseautés) : EasyUV cherche l'axe que toutes les normales coupent (surface de révolution) et vérifie que le profil a des portions droites (paroi, cône, disque). Ces objets étaient classés « organique » dès que leurs arêtes étaient arrondies ; ils sont maintenant dépliés en bandes droites + couvercles. Sur un pot de crème biseauté : distorsion 0,064 -> 0,007. Les sphères (axe non unique) et les tores (profil entièrement courbe) restent en organique.

### Interface
- Messages de l'éditeur UV lisibles sur le damier (fond sombre).
- Pendant un dépliage ou un rangement, l'éditeur UV affiche « Dépliage en cours… » avec un indicateur.
- **F** cadre la sélection dans la vue sous le pointeur : les îlots sélectionnés dans l'éditeur UV, les objets sélectionnés dans la vue 3D. **Origine** cadre tout.
- La barre d'état affiche les raccourcis de la vue survolée (éditeur UV ou vue 3D).
- Le curseur « Priorité » ne déborde plus du panneau.
- **Numéro de version** dans la barre de titre et la barre de menus, et fenêtre **Aide > Nouveautés** qui liste les changements de chaque version. Elle s'ouvre d'elle-même au premier lancement après une mise à jour.

### Édition UV et rangement (layout)
- **Mode rangement** (Ctrl+P, panneau « Rangement ») : range les UV actuelles dans la tuile sans redéplier, par exemple pour réunir des objets dépliés séparément. Les îlots sélectionnés vont dans la place libre, sans recouvrir les autres. Option « Égaliser la densité ».
- **Édition des îlots dans l'éditeur UV, comme dans Blender** : clic, Maj+clic, sélection au rectangle, A / Alt+A / Ctrl+I, glisser pour déplacer, G / R / S avec X / Y pour l'axe et Ctrl pour le pas, Échap ou clic droit pour annuler, Ctrl+Z. Molette : zoom sous le pointeur ; clic molette ou clic droit glissé : déplacer la vue.
- **Rendre droit** (Maj+W) : une bande courbe de quads devient un rectangle droit, et les couvercles attachés suivent. Aussi : Aligner sur les axes, Miroir, Pivoter de 90°.
- Menu clic droit dans l'éditeur UV.

### Import
- OBJ en Z vers le haut détecté automatiquement : les objets ne sont plus importés couchés.
- Objet → **Pivoter de 90° (redresser)** sur X, Y ou Z, pour les fichiers ambigus.

### Auto-apprentissage
- EasyUV apprend en travaillant. À chaque dépliage, il essaie plusieurs réglages en parallèle (angle de couture, tolérance de distorsion, ARAP), garde le meilleur et mémorise le résultat de chaque essai pour ce type de maillage.
- Les réglages proposés pour un nouveau maillage viennent d'un modèle de substitution à noyau, construit sur l'empreinte du maillage (plis, courbure, bords, topologie, forme). Plus l'outil travaille, mieux il prédit et moins il a besoin d'explorer.
- Curseur **Priorité coutures ↔ distorsion** : il s'applique immédiatement à tout ce qui a déjà été appris, sans réentraînement.
- Modèle partagé entre l'application, `easyuv-cli` et Blender, dans `%APPDATA%\EasyUV\apprentissage.txt` (en ajout seul, sûr à plusieurs). Un modèle de base pré-entraîné est livré.
- CLI : `--auto`, `--train`, `--eval`, `--priority`, `--trials`, `--reset-learning`.

### Corrections
- Dépliage entièrement déterministe : l'ordre aléatoire des tables de hachage influençait le choix des boucles de bord.
- `easyuv-cli --self-update` met à jour `EasyUV.exe` placé à côté du CLI, au lieu de remplacer le CLI lui-même.

## [0.1.0] - 2026-10-21

Première version publique.

- Dépliage UV automatique intelligent : coutures sur les arêtes vives, une seule couture pour les tubes, découpe géodésique guidée par les plis pour les formes fermées, redécoupage tant que la distorsion dépasse la tolérance.
- Dépliage LSCM + relaxation ARAP, empaquetage avec rotation optimale.
- Densité de texels uniforme : « Remplir 0-1 » ou densité cible en px/m.
- Plusieurs objets : rangement dans une tuile commune (même densité) ou une tuile par objet.
- Import OBJ, glTF/GLB, FBX ; export OBJ, GLB, FBX (via Blender).
- Damier couleur avec repère d'orientation, vues Îlots et Étirement.
- Application Windows à l'interface inspirée de Blender, avec mises à jour automatiques.
- `easyuv-cli` pour le traitement par lots.
- Addon Blender 4.2+ / 5.x, mis à jour via le dépôt d'extensions.
