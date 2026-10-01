import { defineConfig } from "vite";
import { cpSync, existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Ajout XIII NRV : embarquer les données que la fenêtre va chercher elle-même.
//
// `src/data` est déclaré dans `tauri.conf.json` comme ressource de l'application,
// mais une ressource n'est lisible que par le code Rust : la fenêtre, elle, ne
// peut aller chercher un fichier que s'il a été copié dans `dist`. Il ne l'était
// pas. En développement le serveur de Vite servait ces fichiers depuis le dossier
// du projet, donc tout marchait ; une fois installée, l'application ne trouvait
// plus une seule traduction et retombait sur les textes anglais écrits en dur
// dans le code. C'est ce que barbaxou a vu le 30/09/2026 après avoir installé
// le programme.
//
// On ne copie que le français et l'anglais : ce sont les deux seules langues
// proposées, et les dix autres pèsent à elles seules une dizaine de mégaoctets.
const LANGUES = ["fr", "en"];

function embarquerLesDonnees() {
  return {
    name: "xiiinrv-embarquer-les-donnees",
    apply: "build",
    closeBundle() {
      const source = "src/data";
      const cible = "dist/src/data";
      if (!existsSync(source)) {
        this.warn(`XIII NRV : ${source} est introuvable, aucune donnée embarquée`);
        return;
      }

      let fichiers = 0;
      let octets = 0;
      const copier = (relatif, racine = source, destination = cible) => {
        const de = join(racine, relatif);
        const vers = join(destination, relatif);
        mkdirSync(dirname(vers), { recursive: true });
        cpSync(de, vers);
        fichiers += 1;
        octets += statSync(de).size;
      };

      for (const nom of readdirSync(source)) {
        if (nom.endsWith(".json")) copier(nom);
      }
      const i18n = join(source, "i18n");
      if (existsSync(i18n)) {
        for (const famille of readdirSync(i18n)) {
          const dossier = join(i18n, famille);
          if (!statSync(dossier).isDirectory()) continue;
          for (const langue of LANGUES) {
            const relatif = join("i18n", famille, `${langue}.json`);
            if (existsSync(join(source, relatif))) copier(relatif);
          }
        }
      }

      // Les images de l'interface — icônes de classe, logo, liens. Même défaut
      // que les traductions, découvert de la même façon : barbaxou a vu les
      // icônes de classe manquer dans l'overlay après installation, alors qu'en
      // développement le serveur de Vite les servait depuis le dossier du
      // projet. `meter.js` les cherche d'abord sous `assets/`, puis sous
      // `src/assets/` ; on remplit les deux, cela pèse 145 Ko.
      const images = 'src/assets';
      if (existsSync(images)) {
        for (const nom of readdirSync(images)) {
          if (!statSync(join(images, nom)).isFile()) continue;
          copier(nom, images, 'dist/assets');
          copier(nom, images, 'dist/src/assets');
        }
      } else {
        this.warn(`XIII NRV : ${images} est introuvable, aucune image embarquée`);
      }

      // Sans les textes de l'interface, l'application s'affiche entièrement en
      // anglais sans rien signaler. Mieux vaut que la construction échoue.
      for (const langue of LANGUES) {
        const attendu = join(cible, "i18n", "ui", `${langue}.json`);
        if (!existsSync(attendu)) {
          this.error(`XIII NRV : ${attendu} manque — l'application serait en anglais`);
        }
      }

      // Et sans les icônes de classe, l'overlay affiche une image cassée à côté
      // de chaque joueur. Même raisonnement : la construction doit échouer.
      const classes = existsSync(images)
        ? readdirSync(images).filter((n) => n.endsWith('.png'))
        : [];
      if (!classes.length) {
        this.error("XIII NRV : aucune image de classe embarquée — l'overlay serait sans icônes");
      }
      for (const nom of classes) {
        if (!existsSync(join('dist/assets', nom))) {
          this.error(`XIII NRV : l'image ${nom} n'a pas été embarquée`);
        }
      }

      console.log(
        `XIII NRV : ${fichiers} fichiers de données embarqués (${(octets / 1e6).toFixed(1)} Mo)`,
      );
    },
  };
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [embarquerLesDonnees()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
