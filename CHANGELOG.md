# Changelog

## [Non publié]

### Dépliage
- **Objets tournés détectés en Auto** (pots, couvercles, bouteilles, tuyaux biseautés) : EasyUV cherche l'axe que toutes les normales coupent (surface de révolution) et vérifie que le profil a des portions droites (paroi, cône, disque). Ces objets étaient classés « organique » dès que leurs arêtes étaient arrondies ; ils sont maintenant dépliés en bandes droites + couvercles. Sur un pot de crème biseauté : distorsion 0,064 -> 0,007. Les sphères (axe non unique) et les tores (profil entièrement courbe) restent en organique.

### Interface
- Messages de l'éditeur UV lisibles sur le damier (fond sombre).
- Pendant un dépliage ou un rangement, l'éditeur UV affiche « Dépliage en cours… » avec un indicateur.
- **F** cadre la sélection dans la vue sous le pointeur : les îlots sélectionnés dans l'éditeur UV, les objets sélectionnés dans la vue 3D. **Origine** cadre tout.
- La barre d'état affiche les raccourcis de la vue survolée (éditeur UV ou vue 3D).
- Le curseur « Priorité » ne déborde plus du panneau.

## [0.2.0] - 2026-10-21

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
