# Changelog

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
