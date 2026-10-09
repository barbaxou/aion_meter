# Meter XIII NRV

Meter de combat pour **AION 2**, utilisé par la guilde XIII NRV. Il lit le
trafic réseau du jeu — en lecture seule — et affiche en temps réel les dégâts,
les soins, le déroulé des combats et leur historique.

> **Version modifiée.** Ce programme est un fork d'[A2Tools DPS
> Meter](https://github.com/taengu/A2Tools-DPS-Meter), publié par **taengu**
> sous licence GPL-3.0. Il a été modifié par la guilde XIII NRV à partir du
> **1ᵉʳ octobre 2026**, et continue de l'être. Voir
> [LISEZ-MOI-XIIINRV.md](LISEZ-MOI-XIIINRV.md) pour ce qui change.

---

## Installer le meter

**Tu n'as pas besoin de ce dépôt pour utiliser le meter.** Il contient le code
source ; le programme prêt à installer est sur le site de la guilde.

➜ **Page de téléchargement et notice d'installation : demande le lien à la
guilde.**

La notice complète est aussi lisible ici : [docs/INSTALLATION.md](docs/INSTALLATION.md).
Elle couvre Npcap, l'avertissement Windows, le partage de fiche et le
dépannage.

En deux lignes, pour qui est pressé :

1. Installer **[Npcap](https://npcap.com/#download)** en **décochant**
   « Restrict Npcap driver's access to Administrators only ».
2. Lancer l'installateur `XIII-NRV-Meter_<version>_x64_en-US.msi`, puis
   **lancer le meter avant le jeu**.

---

## Ce que fait le programme

- Overlay en jeu : DPS, dégâts totaux, part des dégâts, soins, Combat Power,
  nombre de morts.
- Barre de vie de la cible, avec le **taux de lecture** — la part des dégâts
  subis que le meter a réellement comptés. Un contrôle de lui-même.
- Fenêtre Détails : par compétence, taux de critique, de dos, de face, coups
  multiples ; onglet séparé pour les soins.
- Historique des combats, sur le disque de l'utilisateur.
- Partage facultatif de la fiche de personnage vers le site de la guilde, avec
  un jeton personnel. Sans jeton, **rien ne part**.

Il ne modifie pas le jeu, n'injecte rien et ne tape aucune touche à la place du
joueur.

---

## Compiler depuis les sources

Prérequis : [Rust](https://rustup.rs), [Node.js](https://nodejs.org) et les
dépendances [Tauri](https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri build
```

L'installateur sort dans `src-tauri/target/release/bundle/msi/`.

Pour lancer la suite de tests :

```bash
cd src-tauri && cargo test
```

Les tests marqués `#[ignore = "diagnostic"]` ne tournent pas par défaut : ce
sont des outils de mesure qui rejouent une capture réelle, lancés
délibérément avec `A2_REPLAY_CAPTURE=... cargo test --test <nom> -- --ignored`.

---

## Organisation du dépôt

| Chemin | Quoi |
|---|---|
| `src-tauri/` | Le cœur, en Rust : capture, décodage des paquets, calculs |
| `public/src/js/` | L'interface : overlay, fenêtre Détails, historique, réglages |
| `src/` | Feuille de style et données du jeu (noms des monstres, compétences, traductions) |
| `docs/INSTALLATION.md` | La notice destinée aux membres |
| `docs/amont/` | Les documents de l'application d'origine, conservés pour l'attribution |

### La méthode

Trois règles, tenues depuis le début. Les commentaires du code en portent la
trace : chaque correction dit ce qui a été mesuré, et avec quels chiffres.

1. **Mesurer avant de corriger.** Une hypothèse non mesurée ne justifie aucun
   changement.
2. **Vérifier qu'un test échoue bien sans la correction.** Sinon il ne prouve
   rien.
3. **Garder les erreurs**, datées. Elles disent ce qui a déjà été essayé.

Le journal de bord complet — une fiche par étape, avec les mesures — est tenu
par la guilde et n'est pas publié ici.

---

## Licence

**GPL-3.0** — voir [LICENSE](LICENSE).

Cette licence donne à quiconque reçoit le programme le droit d'obtenir le code
source de la version exacte qu'il a installée, de l'étudier, de le modifier et
de le redistribuer. Les versions publiées portent un tag (`v2.0.79`, …) pour
que ce source-là soit identifiable.

Le travail d'origine est celui de **taengu** ; les ajouts et corrections de
cette version sont ceux de la guilde XIII NRV. Les mentions de copyright des
fichiers d'origine sont conservées.
