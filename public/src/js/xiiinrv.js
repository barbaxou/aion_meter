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
      if (!e.actif) {
        lignes.push(t("idle", "Partage décoché : rien n'est lu, rien n'est envoyé."));
      } else if (!e.personnage) {
        lignes.push(t("waiting", "En attente : entrez en jeu, le meter lira la fiche de votre personnage."));
      } else {
        lignes.push(`${t("character", "Personnage lu")} : ${e.personnage}`);
        const details = [];
        if (e.combat_power) details.push(`Combat Power ${e.combat_power.toLocaleString("fr-FR")}`);
        if (e.pieces) details.push(`${e.pieces} pièces d'équipement`);
        if (e.pets) details.push(`${e.pets} familles de pets`);
        if (details.length) lignes.push(details.join(" · "));
        // Le nom, le niveau, l'Item Level, le serveur, l'équipement et le Combat
        // Power arrivent à l'entrée en jeu — la fiche y est imbriquée dans un
        // plus gros paquet, et `observer()` va la chercher dedans. Seuls les
        // pets, les PV et les PM attendent l'écran Genus Insight : le jeu ne les
        // envoie qu'à ce moment-là. Mesuré le 05/10/2026 sur l'entrée en jeu du
        // 29/09, sans ouvrir cet écran — tout était lu sauf ces trois-là.
        if (!e.niveau) {
          lignes.push(t("noSheet",
            "La fiche n'a pas encore été lue : entrez en jeu avec ce personnage."));
        } else if (!e.pets) {
          lignes.push(t("noPets",
            "Pets, PV et PM manquants : ouvrez Pets › Genus Insight en jeu. "
            + "Le reste de la fiche est déjà lu."));
        }
        lignes.push(e.jeton_present
          ? `${t("lastSend", "Dernier envoi")} : ${depuis(e.dernier_envoi)}`
          : t("noToken", "Aucun jeton : la fiche est lue, mais rien n'est envoyé."));
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
      caseActif.checked = b.getSetting(CLE_ACTIF) === "true";
    }

    // Ajout XIII NRV : le jeton ne passe plus par `setSetting`.
    //
    // Relevé pendant l'audit du 08/10/2026 : `setSetting` écrit aussi dans le
    // `localStorage` du webview, donc le jeton s'y trouvait en clair, dans un
    // fichier que personne ne pense à regarder. Il a maintenant ses propres
    // commandes, et il ne quitte jamais le processus Rust : cette page sait
    // seulement s'il y en a un, jamais lequel.
    const appelNettoyage = invoke();
    try {
      localStorage.removeItem(CLE_JETON);
    } catch {
      // Un stockage indisponible n'est pas une raison de ne pas continuer.
    }

    const POINTS = "•".repeat(16);
    const montrerEtatJeton = async () => {
      const appel = invoke();
      if (!appel) return;
      try {
        const present = await appel("xiiinrv_jeton_present");
        champJeton.value = present ? POINTS : "";
        champJeton.placeholder = present
          ? t("tokenSet", "Jeton enregistré — collez-en un autre pour le remplacer")
          : t("tokenNone", "Collez ici le jeton donné par la guilde");
      } catch {
        // Rien à afficher : on laisse le champ tel quel.
      }
    };
    void appelNettoyage;
    void montrerEtatJeton();

    // Le jeton n'est enregistré qu'à la sortie du champ : on évite d'écrire un
    // jeton incomplet à chaque frappe. Les points affichés ne sont pas un
    // jeton : les retaper tels quels ne doit rien changer.
    champJeton.addEventListener("focus", () => {
      if (champJeton.value === POINTS) champJeton.value = "";
    });
    champJeton.addEventListener("change", async () => {
      const saisi = champJeton.value.trim();
      if (saisi === POINTS) return;
      const appel = invoke();
      if (appel) await appel("xiiinrv_enregistrer_jeton", { jeton: saisi }).catch(() => {});
      await montrerEtatJeton();
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
