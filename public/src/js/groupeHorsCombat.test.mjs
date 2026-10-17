// Test de l'affichage du groupe hors combat : node public/src/js/groupeHorsCombat.test.mjs
import vm from "node:vm";
import assert from "node:assert/strict";
import fs from "node:fs";

const contexte = { module: { exports: {} } };
vm.runInNewContext(fs.readFileSync(new URL("./groupeHorsCombat.js", import.meta.url), "utf8"), contexte);
const { remplacerParLeGroupe, libelleSalon } = contexte.module.exports;

const ligne = (name, totalDamage = 0, dps = 0) => ({ name, totalDamage, dps });
const ancienGroupe = [ligne("JoueurA"), ligne("JoueurB"), ligne("JoueurC")];
const nouveauGroupe = [ligne("JoueurD"), ligne("JoueurE"), ligne("JoueurF")];

// 17/10/2026 : le nouveau groupe remplace l'ancien tant qu'aucun des deux n'a frappé.
assert.deepEqual(remplacerParLeGroupe(nouveauGroupe, ancienGroupe), nouveauGroupe);

// 17/10/2026 : le groupe du salon remplace aussi un combat terminé (rendu avec dégâts).
const combatFini = [ligne("JoueurA", 1_200_000, 9000), ligne("JoueurB", 900_000, 7000)];
assert.deepEqual(remplacerParLeGroupe(nouveauGroupe, combatFini), nouveauGroupe);

// Des lignes qui portent des dégâts ne remplacent rien non plus (nouveau combat en cours).
assert.equal(remplacerParLeGroupe([ligne("JoueurD", 50, 5)], ancienGroupe), null);

// Rien à afficher, ou rien de précédent : on ne décide pas ici.
assert.equal(remplacerParLeGroupe([], ancienGroupe), null);
assert.equal(remplacerParLeGroupe(nouveauGroupe, []), null);
assert.equal(remplacerParLeGroupe(nouveauGroupe, null), null);

console.log("groupeHorsCombat : ok");

// 17/10/2026 : titre du salon d'attente, comme Abyss (« Salon · Donjon · 1/4 prêts »).
const fr = "Salon · {prets}/{total} prêts";
assert.equal(libelleSalon({ prets: 1, total: 4, modele: fr }), "Salon · 1/4 prêts");
assert.equal(libelleSalon({ prets: 1, total: 4, donjon: "Base de recherche", modele: fr }), "Salon · Base de recherche · 1/4 prêts");
assert.equal(libelleSalon({ prets: 0, total: 3 }), "Salon · 0/3 ready", "modèle par défaut en anglais");
assert.equal(libelleSalon({ prets: -1, total: -1, modele: fr }), null, "aucun salon annoncé");
assert.equal(libelleSalon({ prets: 2, total: 4, modele: fr, cible: true }), null, "un combat en cours prend le titre");
console.log("groupeHorsCombat : ok");
