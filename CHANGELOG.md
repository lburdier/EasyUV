# Changelog

## [0.2.0] - 2026-10-21

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
