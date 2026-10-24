# Changelog

## [0.3.9] - 2026-10-25

### Rendre droit : corrections
- **Rendre droit, Ligne horizontale et Ligne verticale marchent sur les vraies lignes** : sur Suzanne, la plupart étaient refusées (« des faces se seraient retournées »). Les faces autour de la ligne sont maintenant recalculées d'après la 3D (LSCM épinglé sur la ligne), d'abord près d'elle puis sur tout l'îlot s'il le faut.
- La ligne redressée **garde sa longueur** au lieu d'être écrasée entre ses deux bouts.
- Une ligne trop courbe pour son îlot est quand même redressée, au mieux, avec un message qui compte les faces pliées (`Ctrl+Z` pour annuler) ; une boucle fermée est refusée avec une explication.
- **En mode îlots**, Rendre droit fait comme `Maj+W` (bandes, arcs) au lieu d'échouer.
- **Avec un pinceau actif**, le double-clic sur une arête sélectionne sa boucle, comme avec l'outil Sélection.

## [0.3.8] - 2026-10-24

### Sélection par zone dans la vue 3D
- **Glisser au clic gauche dans la vue 3D sélectionne par zone** : rectangle, lasso ou cercle, la même forme que dans l'éditeur UV (`W` pour changer, `[` `]` : rayon du cercle). `Maj` ajoute, `Ctrl` retire.
- En mode Objet, les objets vus dans la zone ; en Édition, les sommets, arêtes, faces (centre dans la zone) ou îlots, seulement ceux qui sont visibles (pas ceux cachés derrière le modèle).
- **Navigation** : la vue tourne avec `Alt`+glisser ou la molette cliquée ; le clic droit glissé (ou `Maj` + molette cliquée) la déplace toujours.

## [0.3.7] - 2026-10-24

### Lisser et rendre droit à l'endroit choisi
- **Lisser** la sélection (sommets, arêtes, faces ou îlots) : les arêtes reprennent leur longueur 3D, les ondulations disparaissent ; le reste ne bouge pas.
- **Rendre droit** une ligne de sommets ou d'arêtes (une boucle, par exemple), ou des faces, qui se rangent en grille droite.
- **Ligne horizontale / verticale** : la ligne choisie devient droite et se couche sur U ou sur V.
- Les faces autour suivent la ligne redressée sans se retourner ; une opération qui abîmerait des faces est annulée. La zone isolée (`H`) est respectée.
- Dans le panneau *Outils UV* (rangée *Lignes et faces*), le menu *UV* et le clic droit de l'éditeur UV.

## [0.3.6] - 2026-10-24

### Isoler une zone des UV
- Les pinceaux (Déplacer, Détendre, Redresser, Pincer) ne modifient que la **sélection** : un îlot, des faces ou des sommets. Ce qui ne bougera pas est estompé dans l'éditeur UV.
- **`H` isole la sélection** (menu *UV* ou bouton *Isoler* du panneau) : seule cette zone bouge, au pinceau comme avec `G` / `R` / `S`, même après avoir changé de sélection. `Alt+H` (ou *Libérer*) rend tout modifiable.
- Les sommets partagés avec une face hors de la zone restent figés : les faces voisines ne sont jamais déformées.

## [0.3.5] - 2026-10-23

### Modes Objet et Édition comme Blender
- `Tab` (ou le bouton Objet / Édition) ne passe en **Édition** que les objets sélectionnés en mode Objet ; sans objet sélectionné, on reste en mode Objet (un modèle à un seul objet y passe directement).
- En Édition, seuls les objets édités montrent leur fil de fer, leurs sommets et leur sélection, et seuls leurs sommets, arêtes et faces se choisissent (clic, `Alt`+clic, `A`, `L`, `Ctrl+I`…, dans la vue 3D comme dans l'éditeur UV). Les autres objets restent affichés tels quels.
- Le contour orange des objets disparaît en Édition ; le clic droit n'y change plus l'objet sélectionné.
- Le bouton Objet / Édition reste visible quand la vue 3D est étroite (disposition Layout) : le titre s'efface, puis le bouton passe en version courte. La barre d'état rappelle `Tab : Édition`.

## [0.3.4] - 2026-10-23

### Sélection directe dans la vue 3D
- Choisir le mode sommets, arêtes ou faces (dans l'éditeur UV ou la vue 3D) passe la vue 3D en **Édition** d'office : un clic sur le modèle choisit directement l'élément, sans passer par `Tab`. `Tab` revient au mode Objet.
- Bouton Objet / Édition plus visible dans l'en-tête de la vue 3D (orange en Édition), placé juste après le titre quelle que soit sa longueur.
- L'en-tête de l'éditeur UV ne cache plus son titre sous les icônes quand il est étroit.


## [0.3.3] - 2026-10-23

### Vue 3D comme Blender
- **Vues d'axe** : un clic sur une boule du gizmo (X, Y, Z ou leurs opposées) place la vue sur cet axe, en orthographique (Droite, Gauche, Face, Arrière, Dessus, Dessous). Pavé numérique : `1` face, `3` droite, `7` dessus, `Ctrl` pour le côté opposé, `5` perspective / orthographique. Tourner la vue rend la perspective. L'en-tête affiche la vue en cours.
- **Mode Édition** (`Tab`) : on choisit **sommets, arêtes et faces directement sur le modèle 3D** (`1` `2` `3`, `4` îlots), `Maj`+clic pour ajouter ou retirer, **`Alt`+clic pour une boucle d'arêtes**. La sélection est partagée avec l'éditeur UV, et marche avant tout dépliage (pour placer ses coutures d'abord).
- Raccourcis Blender en Édition : `A` / `Alt+A` tout / rien, `Ctrl+I` inverser, `L` partie reliée sous le pointeur, `Ctrl+L` tout ce qui est relié, `Ctrl+` / `Ctrl-` agrandir / réduire, `Ctrl+E` marquer la couture, `Maj+Ctrl+E` la retirer, `U` déplier la sélection. Fil de fer et sommets affichés en Édition.

### Affichage
- **Image nette en mouvement** : la vue 3D ne passe plus d'office en demi-résolution quand on tourne ou zoome. Le temps de chaque image est mesuré et la résolution ne baisse que si une image pleine résolution dépasserait ~14 ms (plus de 60 images par seconde) ; les modèles courants restent nets.


## [0.3.2] - 2026-10-23

### Projeter depuis la vue
- **Dépliage selon l'angle de vue** (comme « Project From View » de Blender) : les UV prennent exactement ce que l'on voit dans la vue 3D, sous l'angle choisi. Idéal pour un logo, une étiquette ou un décor à peindre de face.
- S'applique aux faces sélectionnées dans l'éditeur UV, sinon aux objets sélectionnés, sinon à tout. Le résultat est rangé dans la place libre à la densité des autres îlots ; la variante **Cadré** l'étire sur toute la tuile.
- Bouton dans l'en-tête de la vue 3D et dans le panneau « Dépliage intelligent », entrées dans le menu UV, le menu Objet et le clic droit.


## [0.3.1] - 2026-10-23

### Formes organiques (crosses, sculptures)
- **Coutures sans escaliers** : quand un îlot est redécoupé, la frontière entre les deux moitiés est optimisée face par face (longueur + pénalité à chaque virage). Sur une grille de quads dense, les coutures suivent les boucles d'arêtes au lieu de zigzaguer : virages des coutures -43 % en moyenne sur le corpus de test, -56 % sur une forme organique en quads.
- **Moins de miettes** : les petits îlots voisins sont fusionnés jusqu'à 3 % de la surface (au lieu de 1 %) quand la qualité le permet ; -10 % d'îlots sur le corpus, distorsion inchangée. Sur la crosse du revolver de test : 180 -> 159 îlots.
- Un îlot replié sur lui-même est d'abord détendu plus longtemps avant d'être redécoupé.

### Rendre droit, une bonne fois pour toutes
- **Grille parfaite** (`Maj+G`) : chaque rangée et chaque colonne de l'îlot passe sur une droite, comme tracée à la règle. Rangées et colonnes sont lues face par face (les diagonales des triangles ne comptent pas), les rangées fines d'un biseau restent distinctes, l'orientation de l'îlot est trouvée seule. Sur la sélection, ou sur tous les îlots ; un îlot qui en serait abîmé reste tel quel.
- « Rendre droit » finit désormais par la Grille parfaite : les lignes intérieures ne gondolent plus.
- **Remettre d'aplomb** : chaque îlot tourne pour que ses arêtes suivent l'horizontale et la verticale (d'après ses arêtes, pas sa boîte), côté long horizontal.

### Sélection
- **Sélection par zone** : rectangle, **lasso** (tracé à main levée) ou **cercle** que l'on peint ; `W` passe de l'une à l'autre, comme Blender. Icônes dans l'en-tête de l'éditeur UV.
- **`Maj` maintenu, comme Blender** : `Maj`+clic ajoute ou retire, `Maj`+glisser ajoute par zone même en partant d'un îlot (au lieu de le déplacer), `Ctrl`+glisser retire.
- **Sélection par angle** (seuil réglable) : en mode arêtes, les arêtes vives (pour Couper ou Marquer) ; dans les autres modes, toutes les faces d'un même pan à partir des faces choisies.


## [0.3.0] - 2026-10-23

### Sélection de sommets, d'arêtes et de faces (comme Blender)
- Modes de sélection de l'éditeur UV : **sommets** `1`, **arêtes** `2`, **faces** `3`, **îlots** `4`, aussi en icônes dans l'en-tête de l'éditeur. La sélection est convertie quand on change de mode.
- Clic, `Maj`+clic (ajouter ou retirer), rectangle, `A` / `Alt+A` / `Ctrl+I` ; glisser ou `G` `R` `S` déplacent, pivotent et mettent à l'échelle la sélection dans tous les modes. Les deux côtés d'une couture restent séparés (comme « Shared Location » de Blender).
- **Double-clic sur une arête : toute la boucle d'arêtes.**
- Vue 3D : arêtes choisies en orange, faces choisies teintées, coutures marquées en rose.

### Dépliage de la sélection
- **`U` avec une sélection dans l'éditeur UV ne déplie qu'elle** (îlots ou faces) : dépliage intelligent des seules faces choisies, rangées dans la place libre à la densité des autres îlots ; le reste ne bouge pas. `Maj+U` déplie toujours les objets.

### Outils de couture
- **Couper** : les arêtes sélectionnées deviennent des coutures et les îlots traversés sont redépliés.
- **Coudre** : l'îlot voisin est tourné et rapproché, les arêtes sont soudées, puis l'îlot recousu est détendu (« Move and Sew » de Maya).
- **Marquer la couture / Retirer la marque** : coutures imposées à tous les dépliages suivants (« Mark Seam » de Blender), gardées à la suppression ou à la duplication d'objets.

### Score UV en direct
- Note sur 100 dans l'en-tête de l'éditeur UV et dans le panneau « Score UV », recalculée à chaque modification (même pendant un coup de pinceau) : distorsion, chevauchements, faces retournées, remplissage de la tuile, UV hors tuile, densité égale entre îlots, coutures. Une barre par critère et un conseil pour le point le plus faible.

### Rendre droit : toutes les bandes
- Nouvelle méthode « rectangle à bord épinglé » (comme RizomUV ou Maya) : les 4 coins de la bande sont trouvés seuls, son contour est posé sur un rectangle aux longueurs 3D, l'intérieur suit par LSCM. Les bandes ondulées, irrégulières, avec triangles ou jonctions en T deviennent droites, là où ni la grille de quads ni le déroulé en arc ne suffisaient. Parmi les méthodes qui n'abîment pas l'îlot, la moins déformée l'emporte.
- **Coins choisis à la main** : en mode sommets, sélectionnez les 4 coins de la bande puis `Maj+W` ; ils deviennent les coins du rectangle.
- Les outils d'îlots (pivoter, miroir, orienter, rendre droit…) marchent aussi en mode sommets, arêtes et faces, sur les îlots de la sélection.

### Sélection visible sur le modèle 3D
- Les îlots sélectionnés dans l'éditeur UV sont teintés en orange sur le modèle 3D, et l'îlot survolé s'éclaire en jaune : on voit tout de suite de quelle partie du modèle il s'agit.

### Damiers
- Sept textures de contrôle (panneau « Damier », aussi dans Blender) : Couleur, Mono, **Numéroté** (A1, B2… dans chaque case), **Cercles** (les ellipses montrent l'étirement), **Flèches** (rotations et miroirs), **Dégradé UV** (rouge = U, vert = V), **Diagonales** (cisaillement).

### Icônes et aperçus
- Chaque outil et chaque mode a son icône. En laissant le pointeur dessus, une infobulle explique l'outil avec une petite animation de ce qu'il fait.

### Pinceaux UV
- Nouveaux outils dans l'éditeur UV (touches `Q`, `Maj+1` à `Maj+5`, ou panneau « Outils UV ») :
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
