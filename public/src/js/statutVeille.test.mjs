// Test de la ligne de statut : node public/src/js/statutVeille.test.mjs
import vm from "node:vm";
import assert from "node:assert/strict";
import fs from "node:fs";

const contexte = { module: { exports: {} } };
vm.runInNewContext(fs.readFileSync(new URL("./statutVeille.js", import.meta.url), "utf8"), contexte);
const { texteOde, texteGs, texteCp, iconeClasse, nombreDeValeurs, valeursVisibles } = contexte.module.exports;

// L'ode : le courant, puis le bonus entre parenthèses.
assert.equal(texteOde({ courant: 75, bonus: 90 }), "75 (+90)");
assert.equal(texteOde({ courant: 215, bonus: 30 }), "215 (+30)");
assert.equal(texteOde({ courant: 75, bonus: 0 }), "75");
assert.equal(texteOde(null), "", "liste des tickets pas encore lue");
assert.equal(texteOde({ courant: "x" }), "");

// L'icône : le nom de classe, jamais un chemin.
assert.equal(iconeClasse("정령성"), "./assets/정령성.png");
assert.equal(iconeClasse(null), null);
assert.equal(iconeClasse("../secret"), null);
assert.equal(iconeClasse("  "), null);

// 1, 2 ou 3 valeurs ; tout le reste retombe sur 2, comme avant.
assert.equal(nombreDeValeurs(1), 1);
assert.equal(nombreDeValeurs("3"), 3);
assert.equal(nombreDeValeurs(7), 2);
assert.equal(nombreDeValeurs(undefined), 2);
const v = (n) => JSON.stringify(valeursVisibles(n));
assert.equal(v(1), JSON.stringify({ secondaire: false, tertiaire: false }));
assert.equal(v(2), JSON.stringify({ secondaire: true, tertiaire: false }));
assert.equal(v(3), JSON.stringify({ secondaire: true, tertiaire: true }));

// Gear Score et Combat Power : compacts, vides tant qu'ils ne sont pas lus.
assert.equal(texteGs(1580), "1580");
assert.equal(texteGs(null), "");
assert.equal(texteGs(0), "");
assert.equal(texteCp(88774), "88.8K");
assert.equal(texteCp(120300), "120.3K");
assert.equal(texteCp(950), "950");
assert.equal(texteCp(1_250_000), "1.25M");
assert.equal(texteCp(undefined), "");

console.log("statutVeille : ok");
