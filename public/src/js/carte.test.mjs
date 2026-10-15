// Test de la projection de la carte : node public/src/js/carte.test.mjs
import vm from "node:vm";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
// carte.js est un script de navigateur : on le charge avec un `module` factice.
const contexte = { module: { exports: {} } };
vm.runInNewContext(fs.readFileSync(new URL("./carte.js", import.meta.url), "utf8"), contexte);
const { projeter, CALQUES, classer, confirme } = contexte.module.exports;
const ctxCat = { module: { exports: {} } };
vm.runInNewContext(fs.readFileSync(new URL("./carteCategories.js", import.meta.url), "utf8"), ctxCat);
const { CATEGORIES, categoriser, compter } = ctxCat.module.exports;

const plat = (v) => JSON.parse(JSON.stringify(v));
const carte = { pixelSize: 4096, world: { minX: -408000, maxX: 408000, minY: -408000, maxY: 408000 } };
// Les coins des bornes tombent sur 0 et pixelSize (coordonnées en mètres).
assert.deepEqual(plat(projeter(carte, -4080, -4080)), { x: 0, y: 0 });
assert.deepEqual(plat(projeter(carte, 4080, 4080)), { x: 4096, y: 4096 });
assert.deepEqual(plat(projeter(carte, 0, 0)), { x: 2048, y: 2048 });
// Sans bornes (Abyss_Reshanta_B) : pas de position.
assert.equal(projeter({ pixelSize: 2048 }, 1, 1), null);
// Les objets vus ont leurs calques ; les positions du jeu sont en centimètres.
assert.deepEqual(plat(CALQUES.map((c) => c.id)), ["fond", "chemins", "lieux"], "les objets se filtrent par catégorie, plus par calque");
assert.deepEqual(plat(projeter(carte, 110192 / 100, 76199 / 100)), { x: 2048 + 110192 / 408000 * 2048, y: 2048 + 76199 / 408000 * 2048 });

// Sur le cache réel, s'il est là : tous les lieux d'Altgard tombent dans l'image.
const reel = path.join(os.tmpdir(), "xiiinrv-fonds-reel", "carte", "carte.json");
if (fs.existsSync(reel)) {
  const donnees = JSON.parse(fs.readFileSync(reel, "utf8"));
  const c = donnees.maps.World_D_A;
  assert.ok(c.poi.length > 0);
  for (const p of c.poi) {
    const pos = projeter({ ...c, unitFactor: donnees.unitFactor }, p.x, p.y);
    assert.ok(pos.x >= 0 && pos.x <= c.pixelSize && pos.y >= 0 && pos.y <= c.pixelSize, p.name);
  }
  console.log(`réel : ${c.poi.length} lieux d'Altgard dans l'image`);
} else console.log("cache réel absent : contrôle sur données réelles ignoré");

// Cercle vert : « confirmé actif » seulement si le meter vient de voir l'objet.
{
  const t = 1_700_000_000_000;
  assert.equal(confirme({ genre: "ephemere", vu_a: 0 }, t), true, "un éphémère présent est actif");
  assert.equal(confirme({ genre: "decor", vu_a: t - 60_000 }, t), true, "vu il y a 1 min");
  assert.equal(confirme({ genre: "decor", vu_a: t - 3_600_000 }, t), false, "vu il y a 1 h : non confirmé");
  assert.equal(confirme({ genre: "decor" }, t), false, "sans date : non confirmé");
}

// Catégories sans référentiel : toujours contrôlées, même sans les données du site.
assert.equal(categoriser(null, 1300908), "autre", "sans référentiel");
assert.equal(categoriser({}, 9999999), "autre", "modèle inconnu");
assert.equal(categoriser({}, 0, "ephemere"), "ame");
assert.equal(categoriser({}, 0, "mob"), "monstre");
assert.equal(categoriser({}, 2950007, "mob", { name: "x", isBoss: false }), "monstre");
assert.equal(categoriser({}, 2950007, "mob", { name: "x", isBoss: true }), "boss");
assert.equal(categoriser({}, 2400032, "mob", { name: "mannequin", isBoss: true, isDummy: true }), "monstre", "un mannequin n'est pas un boss");
const faux = {
  1: { name: "Faux cube", internal: "E_Test", usage: "Normal", category: "HiddenCubeDark" },
  2: { name: "Faux minerai", internal: "Gather_Source_Ore_L_lv1", usage: "GatherSource", category: "None" },
  3: { name: "Fausse porte", internal: "E_Porte", usage: "EnterDungeon", category: "None" },
  4: { name: null, internal: "E_SansNom", usage: "Quest", category: "None" },
};
assert.equal(categoriser(faux, 1), "cube");
assert.equal(categoriser(faux, 2), "minerai");
assert.equal(categoriser(faux, 3), "portail");
assert.equal(categoriser(faux, 4), "quete");
{
  const ids = CATEGORIES.map((c) => c.id);
  assert.equal(new Set(ids).size, ids.length, "identifiants de catégorie uniques");
  for (const c of CATEGORIES) assert.ok(c.couleur && c.picto && c.libelle, c.id);
  const n = compter([{ genre: "ephemere", modele: 0 }, { genre: "decor", modele: 1 }, { genre: "decor", modele: 3 }, { genre: "decor", modele: 3 }], faux);
  assert.deepEqual(plat(n), { ame: 1, cube: 1, portail: 2 });
}

// Noms et filtre : sur les vraies données du site, si elles sont là.
const site = "D:/6_projets_IA/1_aion/2_site/data/jeu/envobjs.json";
if (fs.existsSync(site)) {
  const envobjs = JSON.parse(fs.readFileSync(site, "utf8")).envobjs;
  const porte = classer(envobjs, 1300908); // « Abandoned Campsite Door », sur la carte du jeu
  assert.equal(porte.nom, "Abandoned Campsite Door");
  assert.equal(porte.surCarteDuJeu, true);
  const piano = classer(envobjs, 1100356); // piano, LandingSensor
  assert.equal(piano.bruit, true);
  const arrivee = classer(envobjs, 1100001); // « Arrival Point », TriggerActor
  assert.equal(arrivee.bruit, true);
  // Un nœud de récolte absent de la carte du jeu reste visible.
  const horsCarte = Object.entries(envobjs).find(([, v]) => v.usage === "GatherSource" && !v.onWorldMap && !v.onMiniMap);
  assert.ok(horsCarte, "il existe des GatherSource hors carte");
  assert.equal(classer(envobjs, Number(horsCarte[0])).surCarteDuJeu, false);
  // Un modèle inconnu est affiché : il peut valoir le coup.
  const inconnu = classer(envobjs, 9999999);
  assert.equal(inconnu.inconnu, true);
  // `name` vaut parfois null : pas de plantage, un nom de repli.
  const sansNom = Object.entries(envobjs).find(([, v]) => v.name == null);
  if (sansNom) assert.match(classer(envobjs, Number(sansNom[0])).nom, /\S/);
  // Sans données du tout : tout est affiché, rien n'est nommé.
  assert.equal(classer(null, 1300908).inconnu, true);
  console.log("noms et filtre : ok");

  // Catégories : d'après le nom interne et l'usage de `envobjs.json`.
  const parInterne = (motif) => Object.entries(envobjs).filter(([, v]) => v.usage === "GatherSource" && motif.test(v.internal));
  const attendu = [
    [/Gemstone|Jewelry/i, "gemme"], [/Ore/, "minerai"], [/Herb|Plant/i, "herbe"],
    [/Wood|Tree/i, "bois"], [/Cotton/i, "coton"], [/Food|Vegetable|Shell|Fish/i, "nourriture"], [/(^|_)Od(_|[A-Z]|$)/, "odyle"],
  ];
  for (const [motif, cat] of attendu) {
    const lot = parInterne(motif);
    assert.ok(lot.length > 0, `des GatherSource ${cat} existent`);
    for (const [id] of lot) assert.equal(categoriser(envobjs, Number(id)), cat, `${envobjs[id].internal} → ${cat}`);
  }
  const recoltes = Object.entries(envobjs).filter(([, v]) => v.usage === "GatherSource" && !/^Test_/.test(v.internal));
  const sansCategorie = recoltes.filter(([id]) => categoriser(envobjs, Number(id)) === "autre");
  assert.equal(sansCategorie.length, 0, "récoltes sans catégorie : " + sansCategorie.map(([, v]) => v.internal).join(", "));
  const cubes = Object.entries(envobjs).filter(([, v]) => String(v.category).startsWith("HiddenCube"));
  assert.ok(cubes.length >= 19);
  for (const [id] of cubes) assert.equal(categoriser(envobjs, Number(id)), "cube");
  const premier = (usage) => Number(Object.entries(envobjs).find(([, v]) => v.usage === usage)[0]);
  assert.equal(categoriser(envobjs, premier("Quest")), "quete");
  assert.equal(categoriser(envobjs, premier("EnterDungeon")), "portail");
  assert.equal(categoriser(envobjs, premier("TeleportArtifact")), "teleporteur");
  assert.equal(categoriser(envobjs, 9999999), "autre", "modèle inconnu");
  assert.equal(categoriser(null, 1300908), "autre", "sans référentiel");
  assert.equal(categoriser(envobjs, 0, "ephemere"), "ame");
  assert.equal(categoriser(envobjs, 0, "mob"), "monstre");
  // Chaque catégorie a une couleur et un pictogramme, et les identifiants sont uniques.
  const ids = CATEGORIES.map((c) => c.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const c of CATEGORIES) assert.ok(c.couleur && c.picto && c.libelle, c.id);
  // Les compteurs comptent par catégorie.
  const lot = [
    { genre: "decor", modele: 1300908 }, { genre: "decor", modele: 1300908 },
    { genre: "ephemere", modele: 0 }, { genre: "decor", modele: Number(cubes[0][0]) },
  ];
  const n = compter(lot, envobjs);
  assert.equal(n.ame, 1);
  assert.equal(n.cube, 1);
  assert.equal(n.portail, 2);
  console.log("catégories : ok");
} else console.log("envobjs.json du site absent : contrôle des noms ignoré");
console.log("ok");
