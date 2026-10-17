// Ligne de statut et nombre de valeurs de l'overlay (ajout XIII NRV).
//
// Hors combat, le pied de l'overlay montre : icône de classe, pseudo, ode (l'Odyle, « 75 (+90) »)
// et ping. Ce module ne contient que la logique pure, testée sous Node
// (`statutVeille.test.mjs`) ; le branchement au DOM est dans core.js.
(function (racine) {
  "use strict";

  const entier = (v) => (Number.isFinite(Number(v)) && Number(v) >= 0 ? Math.trunc(Number(v)) : null);

  // « 75 (+90) », ou « 75 » sans bonus, ou « » tant que la liste des tickets n'est pas lue.
  function texteOde(ode) {
    const courant = entier(ode?.courant);
    if (courant === null) return "";
    const bonus = entier(ode?.bonus);
    return bonus ? `${courant} (+${bonus})` : String(courant);
  }

  // Chemin de l'icône d'une classe (noms coréens, comme sur les lignes), ou null.
  function iconeClasse(job) {
    const nom = typeof job === "string" ? job.trim() : "";
    if (!nom || /[\/.]/.test(nom)) return null;
    return `./assets/${nom}.png`;
  }

  // Gear Score : l'entier tel quel (1580), ou « » tant qu'il n'est pas lu.
  function texteGs(valeur) {
    const n = entier(valeur);
    return n === null || n === 0 ? "" : String(n);
  }

  // Combat Power compact, comme sur les lignes : 88774 -> « 88.8K », 1250000 -> « 1.25M ».
  function texteCp(valeur) {
    const n = entier(valeur);
    if (n === null || n === 0) return "";
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2).replace(/\.?0+$/, "")}M`;
    if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
    return String(n);
  }

  // Nombre de valeurs affichées par ligne : 1, 2 ou 3 (2 par défaut, comme avant).
  function nombreDeValeurs(brut) {
    const n = Number(brut);
    return n === 1 || n === 2 || n === 3 ? n : 2;
  }

  // Quelles zones de valeur sont visibles pour ce nombre.
  function valeursVisibles(brut) {
    const n = nombreDeValeurs(brut);
    return { secondaire: n >= 2, tertiaire: n >= 3 };
  }

  const sortie = { texteOde, texteGs, texteCp, iconeClasse, nombreDeValeurs, valeursVisibles };
  if (racine && typeof module !== "undefined" && module.exports) {
    module.exports = sortie;
    return;
  }
  if (typeof window !== "undefined") window.StatutVeille = sortie;
})(typeof module !== "undefined" ? module : null);
