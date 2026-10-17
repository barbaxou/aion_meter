// Quelles lignes montrer quand aucun combat n'est en cours (ajout XIII NRV).
//
// Hors combat (chrono « Inactif »), l'overlay gardait son dernier rendu pour que le
// combat précédent reste lisible, et ignorait les lignes envoyées par le backend. Un
// groupe formé après coup n'apparaissait donc jamais : relevé le 17/10/2026, l'overlay
// montrait encore le groupe précédent alors qu'un nouveau salon était ouvert.
//
// Règle : si ni le dernier rendu ni les nouvelles lignes ne portent le moindre dégât,
// il n'y a rien à garder, c'est un simple roster — on montre le nouveau. Un vrai combat
// terminé reste affiché jusqu'à la remise à zéro.
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
    return sansDegats(nouvelles) && sansDegats(dernier) ? nouvelles : null;
  }

  const sortie = { sansDegats, remplacerParLeGroupe };
  if (racine && typeof module !== "undefined" && module.exports) {
    module.exports = sortie;
    return;
  }
  if (typeof window !== "undefined") window.GroupeHorsCombat = sortie;
})(typeof module !== "undefined" ? module : null);
