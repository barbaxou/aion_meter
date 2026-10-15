// Catégories d'objets de la fenêtre Carte (ajout XIII NRV).
//
// Pur et sans dépendance : chargé avant `carte.js`, et testé sous Node.
// Le classement vient du référentiel `envobjs.json` du site (nom interne et usage
// de chaque modèle) ; les pictogrammes sont dessinés ici, aucune image du jeu
// n'est embarquée.
(function (racine) {
  "use strict";

  // Pictogrammes : tracés SVG 24×24, dessinés au trait.
  const PICTOS = {
    cube: "M12 2 L21 7 V17 L12 22 L3 17 V7 Z M3 7 L12 12 L21 7 M12 12 V22",
    minerai: "M4 18 L7 9 L13 5 L20 11 L18 19 Z M7 9 L12 13 L18 19",
    gemme: "M6 4 H18 L22 10 L12 21 L2 10 Z M2 10 H22 M9 4 L12 10 L15 4",
    herbe: "M5 19 C5 9 12 4 20 4 C20 12 15 19 5 19 Z M5 19 L14 10",
    bois: "M12 3 L18 12 H14 L19 19 H5 L10 12 H6 Z M12 19 V22",
    coton: "M12 9 a3 3 0 1 0 0.01 0 M12 3 V7 M12 17 V21 M3 12 H7 M17 12 H21 M6 6 L9 9 M15 15 L18 18 M18 6 L15 9 M9 15 L6 18",
    nourriture: "M12 7 C6 5 3 10 5 15 C7 20 10 21 12 19 C14 21 17 20 19 15 C21 10 18 5 12 7 Z M12 7 V3",
    odyle: "M12 2 C12 2 5 10 5 15 A7 7 0 0 0 19 15 C19 10 12 2 12 2 Z",
    quete: "M12 4 V14 M12 19 V20",
    portail: "M5 21 V10 A7 7 0 0 1 19 10 V21 M9 21 V12 A3 3 0 0 1 15 12 V21",
    teleporteur: "M12 3 a9 9 0 1 0 0.01 0 M12 8 V16 M8 12 H16",
    ame: "M6 21 V10 A6 6 0 0 1 18 10 V21 L15 18 L12 21 L9 18 Z M9.5 10 H9.6 M14.5 10 H14.6",
    monstre: "M12 3 A8 8 0 0 0 7 17 V21 H17 V17 A8 8 0 0 0 12 3 Z M9 11 H9.1 M15 11 H15.1 M11 16 H13",
    boss: "M3 18 L5 7 L10 12 L12 5 L14 12 L19 7 L21 18 Z",
    autre: "M12 10 a2 2 0 1 0 0.01 0",
  };

  // Ordre d'affichage dans le filtre : ce que la carte du jeu ne donne pas d'abord.
  const CATEGORIES = [
    { id: "cube", libelle: "Cubes cachés", couleur: "#ff7ad9", picto: PICTOS.cube },
    { id: "boss", libelle: "Boss", couleur: "#ff4d4d", picto: PICTOS.boss },
    { id: "monstre", libelle: "Monstres", couleur: "#e8744f", picto: PICTOS.monstre },
    { id: "ame", libelle: "Âmes", couleur: "#c07dff", picto: PICTOS.ame },
    { id: "minerai", libelle: "Minerais", couleur: "#9aa7b8", picto: PICTOS.minerai },
    { id: "gemme", libelle: "Gemmes", couleur: "#4fc3f7", picto: PICTOS.gemme },
    { id: "herbe", libelle: "Herbes", couleur: "#5ed17a", picto: PICTOS.herbe },
    { id: "bois", libelle: "Bois", couleur: "#b8854f", picto: PICTOS.bois },
    { id: "coton", libelle: "Coton", couleur: "#e9e4d4", picto: PICTOS.coton },
    { id: "nourriture", libelle: "Nourriture", couleur: "#ffb347", picto: PICTOS.nourriture },
    { id: "odyle", libelle: "Odyle", couleur: "#ffd24a", picto: PICTOS.odyle },
    { id: "quete", libelle: "Quêtes", couleur: "#4a8cff", picto: PICTOS.quete },
    { id: "portail", libelle: "Portails et donjons", couleur: "#8a6bff", picto: PICTOS.portail },
    { id: "teleporteur", libelle: "Téléporteurs", couleur: "#38d6c4", picto: PICTOS.teleporteur },
    { id: "autre", libelle: "Autres", couleur: "#7d8794", picto: PICTOS.autre },
  ];

  const PORTAILS = ["EnterDungeon", "EnterInstanceLayer", "MoveMap", "DungeonTeleportArtifact"];
  const TELEPORTEURS = ["TeleportArtifact", "Kisk"];

  // Une récolte se range d'après son nom interne (`Gather_Source_Gemstone_L_lv3`…).
  function categorieRecolte(interne) {
    const n = String(interne || "");
    if (/Gemstone|Jewelry/i.test(n)) return "gemme";
    if (/Ore/.test(n)) return "minerai"; // Ore, RareOre, OreIron
    if (/Herb|Plant/i.test(n)) return "herbe";
    if (/Wood|Tree/i.test(n)) return "bois";
    if (/Cotton/i.test(n)) return "coton";
    if (/Food|Vegetable|Shell|Fish/i.test(n)) return "nourriture";
    if (/(^|_)Od(_|[A-Z]|$)/.test(n)) return "odyle"; // Od_, OdSmall
    return "autre";
  }

  // `genre` vaut « decor » (objet du référentiel), « ephemere » (âme), « mob » (monstre).
  // `npc` : la fiche du PNJ (table de noms du meter), pour reconnaître un boss.
  function categoriser(envobjs, modele, genre, npc) {
    if (genre === "ephemere") return "ame";
    if (genre === "mob") return npc && npc.isBoss && !npc.isDummy ? "boss" : "monstre";
    const e = envobjs ? envobjs[String(modele)] : null;
    if (!e) return "autre";
    if (String(e.category || "").startsWith("HiddenCube")) return "cube";
    if (e.usage === "GatherSource") return categorieRecolte(e.internal);
    if (e.usage === "Quest") return "quete";
    if (PORTAILS.includes(e.usage)) return "portail";
    if (TELEPORTEURS.includes(e.usage)) return "teleporteur";
    return "autre";
  }

  // Nombre d'objets par catégorie, pour les compteurs du filtre.
  function compter(objets, envobjs) {
    const n = {};
    for (const o of objets) {
      const c = categoriser(envobjs, o.modele, o.genre);
      n[c] = (n[c] || 0) + 1;
    }
    return n;
  }

  const sortie = { CATEGORIES, PICTOS, categoriser, compter };
  if (racine && typeof module !== "undefined" && module.exports) {
    module.exports = sortie;
    return;
  }
  if (typeof window !== "undefined") window.CarteCategories = sortie;
})(typeof module !== "undefined" ? module : null);
