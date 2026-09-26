// Partage de build vers le site de la guilde XIII NRV.
//
// Cette page ne fait que trois choses : garder le jeton et la case « partager »
// dans les réglages, montrer ce que le meter a lu, et déclencher un envoi
// immédiat. Tout le reste se passe côté Rust (voir src-tauri/src/xiiinrv/).
//
// Sans jeton, rien n'est lu et rien n'est envoyé.

(function () {
  "use strict";

  const CLE_JETON = "xiiinrv_token";
  const CLE_ACTIF = "xiiinrv_partage_build";

  const invoke = () => window.__TAURI__?.core?.invoke;
  const bridge = () => window.javaBridge;
  // Les messages d'état suivent la langue choisie, comme le reste du meter.
  const t = (cle, repli) => window.i18n?.t?.("xiiinrv." + cle, repli) ?? repli;

  function elem(id) {
    return document.getElementById(id);
  }

  function depuis(horodatage) {
    if (!horodatage) return "jamais";
    const minutes = Math.round((Date.now() / 1000 - horodatage) / 60);
    if (minutes < 1) return "à l'instant";
    if (minutes < 60) return `il y a ${minutes} min`;
    const heures = Math.round(minutes / 60);
    if (heures < 24) return `il y a ${heures} h`;
    return `il y a ${Math.round(heures / 24)} j`;
  }

  async function rafraichir() {
    const appel = invoke();
    const zone = elem("xiiinrvEtat");
    if (!appel || !zone) return;
    try {
      const e = await appel("xiiinrv_etat");
      const lignes = [];
      if (!e.jeton_present) {
        lignes.push(t("idle", "Aucun jeton : le partage est à l'arrêt."));
      } else if (!e.actif) {
        lignes.push(t("off", "Jeton enregistré, partage décoché."));
      } else if (!e.personnage) {
        lignes.push(t("waiting", "En attente : entrez en jeu, le meter lira la fiche de votre personnage."));
      } else {
        lignes.push(`${t("character", "Personnage lu")} : ${e.personnage}`);
        const details = [];
        if (e.combat_power) details.push(`Combat Power ${e.combat_power.toLocaleString("fr-FR")}`);
        if (e.pieces) details.push(`${e.pieces} pièces d'équipement`);
        if (e.pets) details.push(`${e.pets} familles de pets`);
        if (details.length) lignes.push(details.join(" · "));
        lignes.push(`${t("lastSend", "Dernier envoi")} : ${depuis(e.dernier_envoi)}`);
      }
      if (e.dernier_message) lignes.push(e.dernier_message);
      zone.textContent = lignes.join("\n");
    } catch (err) {
      zone.textContent = t("unavailable", "État indisponible") + " : " + err;
    }
  }

  function brancher() {
    const champJeton = elem("xiiinrvJeton");
    const caseActif = elem("xiiinrvActif");
    const boutonEnvoi = elem("xiiinrvEnvoyer");
    if (!champJeton || !caseActif || !boutonEnvoi) return;

    const b = bridge();
    if (b) {
      champJeton.value = b.getSetting(CLE_JETON) || "";
      caseActif.checked = b.getSetting(CLE_ACTIF) === "true";
    }

    // Le jeton n'est enregistré qu'à la sortie du champ : on évite d'écrire un
    // jeton incomplet à chaque frappe.
    champJeton.addEventListener("change", () => {
      bridge()?.setSetting(CLE_JETON, champJeton.value.trim());
      rafraichir();
    });
    caseActif.addEventListener("change", () => {
      bridge()?.setSetting(CLE_ACTIF, caseActif.checked ? "true" : "false");
      rafraichir();
    });

    boutonEnvoi.addEventListener("click", async () => {
      const appel = invoke();
      if (!appel) return;
      boutonEnvoi.disabled = true;
      const zone = elem("xiiinrvEtat");
      if (zone) zone.textContent = t("sending", "Envoi en cours…");
      try {
        const message = await appel("xiiinrv_envoyer");
        if (zone) zone.textContent = message;
      } catch (err) {
        if (zone) zone.textContent = String(err);
      } finally {
        boutonEnvoi.disabled = false;
        setTimeout(rafraichir, 1500);
      }
    });

    rafraichir();
    setInterval(rafraichir, 30000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", brancher);
  } else {
    brancher();
  }
})();
