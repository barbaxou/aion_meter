// Quelles lignes montrer quand aucun combat n'est en cours (ajout XIII NRV).
//
// Hors combat (chrono « Inactif »), l'overlay gardait son dernier rendu pour que le
// combat précédent reste lisible, et ignorait les lignes envoyées par le backend. Un
// groupe formé après coup n'apparaissait donc jamais : relevé le 17/10/2026, l'overlay
// montrait encore le groupe précédent alors qu'un nouveau salon était ouvert.
//
// Règle (17/10/2026, décision de barbaxou : le groupe du salon remplace le combat
// précédent, comme Abyss) : des lignes sans le moindre dégât sont un simple roster. Le
// backend ne les envoie ainsi qu'après avoir vidé le combat (autre groupe, remise à zéro) ;
// on les montre donc, même si le dernier rendu portait des dégâts. Un combat terminé reste
// affiché tant que le backend envoie ses lignes avec dégâts. L'historique garde le combat.
//
// Pur et sans dépendance : testé sous Node (`groupeHorsCombat.test.mjs`).
(function (racine) {
  "use strict";

  const nombre = (v) => (Number.isFinite(Number(v)) ? Number(v) : 0);

  function sansDegats(lignes) {
    return Array.isArray(lignes) && lignes.every((l) => nombre(l?.totalDamage) <= 0 && nombre(l?.dps) <= 0);
  }

  // Renvoie les lignes à afficher, ou null pour « garder le rendu actuel ».
  function remplacerParLeGroupe(nouvelles, dernier) {
    if (!Array.isArray(nouvelles) || nouvelles.length === 0) return null;
    if (!Array.isArray(dernier) || dernier.length === 0) return null;
    return sansDegats(nouvelles) ? nouvelles : null;
  }

  const sortie = { sansDegats, remplacerParLeGroupe };
  if (racine && typeof module !== "undefined" && module.exports) {
    module.exports = sortie;
    return;
  }
  if (typeof window !== "undefined") window.GroupeHorsCombat = sortie;
})(typeof module !== "undefined" ? module : null);
