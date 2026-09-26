# Meter XIII NRV — ce que notre version ajoute à A2Tools

Fork d'[A2Tools DPS Meter](https://github.com/taengu/A2Tools-DPS-Meter) (GPL-3.0).
Tout ce qui existait continue de fonctionner à l'identique : overlay, DPS, historique
des combats. On ajoute **une seule chose** : le partage de la fiche de personnage vers
le site de la guilde.

## Ce que ça fait

Quand un membre colle son jeton dans les réglages et coche « Partager ma fiche », le
meter lit quatre paquets du jeu et envoie le résultat au site, au plus une fois toutes
les quinze minutes, et seulement si quelque chose a changé.

| Paquet | Ce qu'on y lit |
|---|---|
| `33 36` | nom du personnage, serveur, niveau, Item Level, PV, PM |
| `11 56` | les objets équipés et leur enchantement |
| `56 36` | le Combat Power |
| `00 90` | les cinq familles de pets et leurs effets |

**Tout le reste est ignoré** : aucun message de discussion, aucune position, aucun autre
joueur. Uniquement la fiche de son propre personnage.

**Aucun nom d'objet n'est lu ni envoyé.** Le jeu ne les fait pas circuler, et le site n'en
veut pas : on envoie l'identifiant officiel de l'objet, son emplacement et son
enchantement. Un objet équipé se reconnaît à sa seule forme dans le trafic — identifiant
de 9 chiffres commençant par 1, 2, 3 ou 8, conteneur `0x0B`, puis l'emplacement. Vérifié
sur deux sessions réelles : 27 objets sur 27, aucun faux positif.

**Sans jeton, ce module ne lit rien et n'envoie rien.** Un interrupteur, fermé par
défaut, n'est ouvert que si un jeton est enregistré **et** que la case « Partager ma
fiche » est cochée. Tant qu'il est fermé, `observer()` ressort à la première ligne :
aucun paquet n'est analysé, rien n'est gardé en mémoire. Si on décoche la case, ce qui
avait été lu est effacé. Le meter se comporte alors exactement comme la version
d'origine.

## Ce qui a été modifié

Trois fichiers ajoutés, quatre points de couture dans le code existant.

**Ajouté**
- `src-tauri/src/xiiinrv/mod.rs` — réglages et présentation du module
- `src-tauri/src/xiiinrv/collecte.rs` — lecture des quatre paquets
- `src-tauri/src/xiiinrv/envoi.rs` — envoi périodique vers le site
- `public/src/js/xiiinrv.js` — le petit panneau dans les réglages

**Modifié**
- `src-tauri/src/lib.rs` — déclaration du module, lecture des réglages au démarrage,
  deux commandes (`xiiinrv_etat`, `xiiinrv_envoyer`), prise en compte d'un changement
  de réglage
- `src-tauri/src/capture/stream_processor.rs` — **une ligne** dans
  `parse_perfect_packet` : `crate::xiiinrv::observer(packet);`
- `index.html` — la section « Guilde XIII NRV » dans le panneau de réglages

Aucune dépendance ajoutée : LZ4, l'envoi HTTP, le JSON et les tâches de fond étaient
déjà là.

## Compiler

Prérequis : **Rust**, **Build Tools C++ 2022**, **Node.js**, et **Npcap** (déjà installé
si Wireshark l'est). Pas besoin du SDK Npcap : `wpcap.dll` est chargée au lancement.

```bash
npm install
npm run tauri dev      # pour essayer
npm run tauri build    # pour produire l'installateur MSI
```

Le meter doit être **lancé en administrateur** pour capturer le trafic.

## Licence

A2Tools est sous GPL-3.0, donc notre version l'est aussi. Si on distribue le programme
compilé aux membres, **on doit rendre ce code source disponible** — concrètement, publier
ce dépôt. Ce n'est pas une option, c'est la contrepartie de pouvoir s'appuyer sur leur
travail.
