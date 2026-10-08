# Meter XIII NRV — installation et utilisation

Guide pour un membre de la guilde. Comptez dix minutes la première fois.

---

## Avant de commencer, trois choses à savoir

**1. Windows va afficher un avertissement.** Le programme n'est pas signé par un
certificat payant, comme la plupart des meters d'AION 2. Windows affichera
« Windows a protégé votre ordinateur ». C'est attendu — la marche à suivre est
plus bas.

**2. Il faut installer Npcap avant le meter.** C'est le composant qui permet de
lire le trafic du jeu. Sans lui, le meter démarre mais n'affiche jamais rien.

**3. Le meter ne modifie pas le jeu.** Il lit le trafic réseau en lecture seule,
comme un moniteur. Il n'injecte rien, ne touche à aucun fichier du jeu et ne
tape aucune touche à votre place.

---

## Étape 1 — Installer Npcap

**Vous l'avez peut-être déjà.** Si vous avez utilisé un meter sur un autre jeu,
Npcap est sans doute installé : passez à l'étape 2, et revenez ici seulement si
le meter vous dit qu'il manque.

1. Allez sur **https://npcap.com/#download** et prenez « Npcap *x.xx* installer ».
2. Lancez-le. À l'écran des options :

   > ### ⚠️ DÉCOCHEZ « Restrict Npcap driver's access to Administrators only »
   >
   > Si vous laissez cette case cochée, il faudra lancer le meter en
   > administrateur **à chaque fois**, sinon il n'affichera rien. C'est la
   > cause numéro un des « ça ne marche pas chez moi ».

3. Laissez les autres options par défaut et terminez l'installation.

Si Npcap est déjà installé mais que vous ne savez plus comment, réinstallez-le
en décochant la case : ça ne casse rien.

---

## Étape 2 — Installer le meter

1. Téléchargez `XIII-NRV-Meter_<version>_x64_en-US.msi` (le lien vous est donné
   par la guilde).
2. Double-cliquez dessus.
3. Si « Windows a protégé votre ordinateur » apparaît :
   **« Informations complémentaires » → « Exécuter quand même »**.
4. Suivez l'installation. Le meter s'installe dans
   `C:\Program Files\XIII NRV Meter`.

Ce que l'installation dépose, et rien d'autre : un programme
(`xiiinrv-meter.exe`, à la racine du dossier), une bibliothèque, et 36 fichiers
de données du jeu — noms des monstres, des compétences, des donjons et les
traductions. 28 Mo en tout.

---

## Étape 3 — Premier lancement

1. **Lancez le meter d'abord**, puis AION 2.

   > Le jeu n'annonce le nom d'un joueur que lorsqu'il a une raison de le
   > faire — à l'apparition, à l'entrée d'un donjon, à un changement de groupe.
   > Un meter démarré en cours de partie a manqué ces annonces et affichera des
   > numéros (`#9635`) à la place de certains pseudos, parfois pendant plusieurs
   > minutes. Démarré avant, il entend tout depuis le début.
   >
   > Le meter attend le jeu sans rien faire tant qu'il n'est pas là.

2. Si le meter annonce que Npcap manque, c'est que l'étape 1 n'a pas abouti.
3. Entrez en jeu avec un personnage. L'overlay se remplit dès les premiers
   coups.

Si rien ne s'affiche après un combat : clic droit sur le meter →
**Exécuter en tant qu'administrateur**. Si ça marche ainsi, c'est que la case
de l'étape 1 est restée cochée — réinstallez Npcap en la décochant, et vous
n'aurez plus besoin des droits administrateur.

---

## Étape 4 — Partager sa fiche (facultatif)

C'est ce qui alimente la page du personnage sur le site de la guilde.

1. Demandez **votre jeton personnel** à un responsable de la guilde.
2. Dans le meter : **Réglages → XIII NRV**, collez le jeton, cochez
   **« Partager ma fiche »**.
3. Allez à la sélection de personnage, puis entrez en jeu : le meter lit la
   fiche à ce moment-là.

**Sans jeton, rien n'est envoyé.** La fiche est lue et affichée dans le meter,
elle ne quitte pas votre machine.

Ce qui part, et rien d'autre : nom du personnage, serveur, niveau, Item Level,
PV, PM, Combat Power, pièces équipées et leur enchantement, familles de pets.
Au plus une fois toutes les quinze minutes, et seulement si quelque chose a
changé. **Aucun nom d'autre joueur ne quitte jamais votre machine.**

---

## Utiliser l'overlay

### Les boutons du bas

| Bouton | Ce qu'il fait |
|---|---|
| Mode de cible | Fait défiler : Boss, Dernier frappé par moi, Entraînement, Toutes les cibles |
| Mesure | Fait défiler la valeur principale : DPS, Dégâts, Part, Soins, Combat Power |
| Remise à zéro | Efface le combat en cours, tout de suite |
| Réduire / Fermer | — |

### Le mode Boss

C'est le mode par défaut. Il ne montre **que** les combats de boss : sur du
trash, l'overlay reste vide, volontairement. Si vous préférez voir tout ce que
vous tapez, passez en **Dernier frappé par moi**.

### Le pourcentage « lus » sur la barre de vie

Au centre de la barre de vie de la cible : la part des dégâts réellement subis
par la cible que le meter a comptés. À 100 %, il n'a rien manqué. Nettement en
dessous, des coups lui échappent — dites-le, c'est une mesure utile.

Il ne vaut que sur une cible que votre groupe affronte seul. Sur un boss de
monde frappé par trente personnes, il affichera 1 % : c'est normal.

### À côté des pseudos

- Le **Combat Power**, quand le jeu l'annonce (membres du groupe uniquement).
- Une **tête de mort** avec un compteur, pour qui est tombé pendant le combat.

### La remise à zéro

Le meter **ne s'efface plus tout seul** à la fin d'un combat : vous avez le
temps de lire les chiffres. Il repart à zéro quand vous engagez une nouvelle
cible, ou quand vous appuyez sur le bouton.

### La fenêtre Détails

Clic sur une ligne de joueur. Deux onglets :

- **Dégâts** : chaque compétence, son nombre de lancers, ses dégâts, ses taux
  de critique, de dos, de face…
- **Soins** : les soins par compétence. Les colonnes propres aux dégâts y sont
  masquées, parce que le jeu ne les envoie pas pour les soins.

Une légende sous le tableau explique chaque colonne.

> **Attention sur les soins** : le chiffre mêle soins et buffs. Le jeu envoie
> les deux dans le même paquet entre alliés, et rien ne permet de les séparer
> pour l'instant.

---

## Mises à jour

Il n'y a **pas** de mise à jour automatique pour l'instant. La guilde annonce
les nouvelles versions ; vous téléchargez le nouveau MSI et vous l'installez
par-dessus — vos réglages et votre historique sont conservés.

> Le vérificateur de mise à jour d'origine a été retiré volontairement : il
> téléchargeait un fichier d'installation depuis un serveur tiers et le lançait
> sans vérifier ni sa signature ni son empreinte. Qui contrôlait ce serveur
> pouvait installer le programme de son choix sur votre machine.

---

## Désinstaller

**Paramètres Windows → Applications → XIII NRV Meter → Désinstaller.**

Vos réglages, votre historique de combats et vos journaux restent dans
`%APPDATA%\gg.xiiinrv.meter`. Supprimez ce dossier si vous voulez tout effacer.

---

## Où sont vos données

| Quoi | Où |
|---|---|
| Réglages, jeton | `%APPDATA%\gg.xiiinrv.meter\settings.json` |
| Historique des combats | `%APPDATA%\gg.xiiinrv.meter\history` |
| Journal de débogage | `%APPDATA%\gg.xiiinrv.meter\debug.log` |

Tout reste sur votre machine. Les journaux de paquets, s'ils sont activés dans
les réglages — ils ne le sont pas par défaut — sont effacés automatiquement au
bout de sept jours.

---

## Origine et licence

Ce meter est une **version modifiée** d'[A2Tools DPS
Meter](https://github.com/taengu/A2Tools-DPS-Meter), publié par taengu sous
licence **GPL-3.0**.

La GPL-3.0 vous donne le droit d'obtenir le **code source** de la version exacte
que vous avez installée, de l'étudier, de le modifier et de le redistribuer.
Demandez-le à la guilde, il vous sera fourni.

Ce que notre version change par rapport à l'originale est écrit dans
[LISEZ-MOI-XIIINRV.md](../LISEZ-MOI-XIIINRV.md), et le détail de chaque
correction dans [docs/suivi/](suivi/).

---

## Ça ne marche pas

| Symptôme | Cause la plus probable |
|---|---|
| L'overlay reste vide après un combat | Npcap installé avec la case « Administrators only » cochée — voir l'étape 1 |
| « Npcap manquant » au démarrage | Npcap pas installé, ou installation interrompue |
| Vide uniquement hors donjon | Normal en mode Boss. Passez en « Dernier frappé par moi » |
| La fiche ne part pas sur le site | Jeton absent ou invalide, ou case « Partager » décochée |
| Windows bloque l'installation | « Informations complémentaires » → « Exécuter quand même » |

Si rien de tout ça : envoyez `%APPDATA%\gg.xiiinrv.meter\debug.log` à la guilde.

Ce qu'il contient, vérifié : **ni votre jeton, ni aucun mot de passe** — en
revanche il contient **votre nom de personnage et ceux des joueurs avec qui vous
avez joué**, des centaines de fois. Transmettez-le en message privé plutôt qu'en
canal public, par égard pour eux.
