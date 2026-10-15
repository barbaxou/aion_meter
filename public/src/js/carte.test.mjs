// Test de la projection de la carte : node public/src/js/carte.test.mjs
import vm from "node:vm";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
// carte.js est un script de navigateur : on le charge avec un `module` factice.
const contexte = { module: { exports: {} } };
vm.runInNewContext(fs.readFileSync(new URL("./carte.js", import.meta.url), "utf8"), contexte);
const { projeter, CALQUES, classer, afficheParDefaut } = contexte.module.exports;

const plat = (v) => JSON.parse(JSON.stringify(v));
const carte = { pixelSize: 4096, world: { minX: -408000, maxX: 408000, minY: -408000, maxY: 408000 } };
// Les coins des bornes tombent sur 0 et pixelSize (coordonnées en mètres).
assert.deepEqual(plat(projeter(carte, -4080, -4080)), { x: 0, y: 0 });
assert.deepEqual(plat(projeter(carte, 4080, 4080)), { x: 4096, y: 4096 });
assert.deepEqual(plat(projeter(carte, 0, 0)), { x: 2048, y: 2048 });
// Sans bornes (Abyss_Reshanta_B) : pas de position.
assert.equal(projeter({ pixelSize: 2048 }, 1, 1), null);
// Les objets vus ont leurs calques ; les positions du jeu sont en centimètres.
assert.deepEqual(plat(CALQUES.filter((c) => c.id === "decor" || c.id === "ephemeres").map((c) => c.disponible)), [true, true]);
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

// Noms et filtre : sur les vraies données du site, si elles sont là.
const site = "D:/6_projets_IA/1_aion/2_site/data/jeu/envobjs.json";
if (fs.existsSync(site)) {
  const envobjs = JSON.parse(fs.readFileSync(site, "utf8")).envobjs;
  const porte = classer(envobjs, 1300908); // « Abandoned Campsite Door », sur la carte du jeu
  assert.equal(porte.nom, "Abandoned Campsite Door");
  assert.equal(porte.surCarteDuJeu, true);
  assert.equal(afficheParDefaut(porte), false, "ce que la carte du jeu affiche déjà est masqué");
  const piano = classer(envobjs, 1100356); // piano, LandingSensor
  assert.equal(piano.bruit, true);
  assert.equal(afficheParDefaut(piano), false, "le bruit est masqué");
  const arrivee = classer(envobjs, 1100001); // « Arrival Point », TriggerActor
  assert.equal(arrivee.bruit, true);
  // Un nœud de récolte absent de la carte du jeu reste visible.
  const horsCarte = Object.entries(envobjs).find(([, v]) => v.usage === "GatherSource" && !v.onWorldMap && !v.onMiniMap);
  assert.ok(horsCarte, "il existe des GatherSource hors carte");
  assert.equal(afficheParDefaut(classer(envobjs, Number(horsCarte[0]))), true);
  // Un modèle inconnu est affiché : il peut valoir le coup.
  const inconnu = classer(envobjs, 9999999);
  assert.equal(inconnu.inconnu, true);
  assert.equal(afficheParDefaut(inconnu), true);
  // `name` vaut parfois null : pas de plantage, un nom de repli.
  const sansNom = Object.entries(envobjs).find(([, v]) => v.name == null);
  if (sansNom) assert.match(classer(envobjs, Number(sansNom[0])).nom, /\S/);
  // Sans données du tout : tout est affiché, rien n'est nommé.
  assert.equal(afficheParDefaut(classer(null, 1300908)), true);
  console.log("noms et filtre : ok");
} else console.log("envobjs.json du site absent : contrôle des noms ignoré");
console.log("ok");
