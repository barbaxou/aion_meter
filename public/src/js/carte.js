// Fenêtre de la carte (ajout XIII NRV).
//
// Lit le cache des fonds de carte (`fonds/carte/…`, téléchargé depuis le site)
// par le protocole `fonds://`. Aucune dépendance : zoom et déplacement sont
// écrits ici. Les calques sont décrits dans CALQUES ; les catégories qui
// viendront du trafic du jeu (boss, failles, ressources, cubes) y figurent déjà,
// grisées, et n'attendent que leur fonction `rendre`.
(function (racine) {
  "use strict";

  // Les positions du jeu sont en centimètres, les lieux de la carte en mètres.
  const CM_PAR_M = 100;

  // Projection du contrat : les lieux sont en mètres, les bornes de monde en
  // centimètres (`unitFactor`). La colonne est X, la ligne est Y (vers le bas).
  function projeter(carte, x, y) {
    const w = carte.world;
    if (!w) return null;
    const f = carte.unitFactor || 100;
    const px = ((x * f - w.minX) / (w.maxX - w.minX)) * carte.pixelSize;
    const py = ((y * f - w.minY) / (w.maxY - w.minY)) * carte.pixelSize;
    return { x: px, y: py };
  }

  const CALQUES = [
    { id: "fond", libelle: "Fond", disponible: true },
    { id: "chemins", libelle: "Chemins", disponible: true },
    { id: "lieux", libelle: "Lieux nommés", disponible: true },
    // Objets vus passer dans le trafic du jeu (décodés par le meter, gardés en local).
    { id: "decor", libelle: "Objets du décor", disponible: true },
    { id: "ephemeres", libelle: "Objets éphémères", disponible: true },
  ];

  // Échelle (écran / pixels de la carte) sous laquelle les noms sont masqués.
  const SEUIL_NOMS = 0.3;

  // Ce que `envobjs.json` (référentiel du site) dit d'un modèle d'objet : son nom, et
  // si la carte du jeu l'affiche déjà. Les modèles absents du référentiel sont
  // « inconnus » et restent affichés : ils peuvent valoir le coup.
  const USAGES_BRUIT = ["LandingSensor", "TriggerActor", "Overlap", "Prop"];

  function classer(envobjs, modele) {
    const e = envobjs ? envobjs[String(modele)] : null;
    if (!e) {
      return { nom: "Modèle " + modele, usage: null, surCarteDuJeu: false, bruit: false, inconnu: true };
    }
    return {
      nom: e.name || e.internal || "Modèle " + modele,
      usage: e.usage || null,
      surCarteDuJeu: !!(e.onWorldMap || e.onMiniMap),
      bruit: USAGES_BRUIT.includes(e.usage),
      inconnu: false,
    };
  }

  // Affiché tant qu'on n'a pas demandé à voir ce que le jeu montre déjà, ni le bruit.
  function afficheParDefaut(c) {
    return c.inconnu || (!c.surCarteDuJeu && !c.bruit);
  }

  const CAMPS = { asmodien: "Asmodiens", elyos: "Élyséens", abysse: "Abysses" };

  if (racine && typeof module !== "undefined" && module.exports) {
    module.exports = { projeter, CALQUES, classer, afficheParDefaut };
    return;
  }
  if (typeof window === "undefined" || window.A2_VIEW !== "carte") {
    window.Carte = { projeter, CALQUES, classer, afficheParDefaut };
    return;
  }

  // ---- Vue ------------------------------------------------------------------

  const BASE = /Windows/i.test(navigator.userAgent) ? "http://fonds.localhost/" : "fonds://localhost/";
  const url = (chemin) => BASE + "carte/" + chemin;
  const CLE_FILTRES = "carte.filtres";
  const CLE_CARTE = "carte.derniere";

  const lire = (cle) => { try { return window.localStorage.getItem(cle); } catch { return null; } };
  const ecrire = (cle, v) => { try { window.localStorage.setItem(cle, v); } catch {} };

  let filtres = {};
  try { filtres = JSON.parse(lire(CLE_FILTRES) || "{}") || {}; } catch { filtres = {}; }
  const actif = (id) => filtres[id] !== false;
  // Les deux cases « réafficher » sont décochées tant qu'on ne les a pas cochées.
  const voir = (id) => filtres[id] === true;
  const EXTRAS = [
    { id: "jeu", libelle: "Déjà sur la carte du jeu" },
    { id: "bruit", libelle: "Bruit (pianos, points d'arrivée)" },
  ];
  const visible = (c) => c.inconnu || ((!c.surCarteDuJeu || voir("jeu")) && (!c.bruit || voir("bruit")));

  const etat = { donnees: null, envobjs: null, id: null, echelle: 1, dx: 0, dy: 0 };
  const el = {};

  function creer(tag, classe, texte) {
    const n = document.createElement(tag);
    if (classe) n.className = classe;
    if (texte != null) n.textContent = texte;
    return n;
  }

  function construire() {
    const panneau = document.querySelector(".cartePanel");
    panneau.textContent = "";
    el.barre = creer("div", "carteBarre");
    el.choix = creer("select", "carteChoix");
    el.statut = creer("span", "carteStatut");
    const fermer = creer("button", "carteFermer", "×");
    fermer.type = "button";
    fermer.title = "Fermer";
    fermer.addEventListener("click", () => window.javaBridge?.closeToolWindow?.());
    el.barre.append(el.choix, el.statut, fermer);

    el.filtres = creer("div", "carteFiltres");
    for (const c of CALQUES) {
      const label = creer("label", "carteFiltre" + (c.disponible ? "" : " aVenir"));
      const case_ = creer("input");
      case_.type = "checkbox";
      case_.checked = c.disponible && actif(c.id);
      case_.disabled = !c.disponible;
      case_.addEventListener("change", () => {
        filtres[c.id] = case_.checked;
        ecrire(CLE_FILTRES, JSON.stringify(filtres));
        appliquerFiltres();
      });
      label.append(case_, document.createTextNode(" " + c.libelle + (c.disponible ? "" : " (à venir)")));
      el.filtres.append(label);
    }

    for (const x of EXTRAS) {
      const label = creer("label", "carteFiltre carteFiltreExtra");
      const case_ = creer("input");
      case_.type = "checkbox";
      case_.checked = voir(x.id);
      case_.addEventListener("change", () => {
        filtres[x.id] = case_.checked;
        ecrire(CLE_FILTRES, JSON.stringify(filtres));
        rafraichirObjets();
      });
      label.append(case_, document.createTextNode(" " + x.libelle));
      el.filtres.append(label);
    }

    el.vue = creer("div", "carteVue");
    el.monde = creer("div", "carteMonde");
    el.vue.append(el.monde);
    el.message = creer("div", "carteMessage");
    el.note = creer("div", "carteNote");
    panneau.append(el.barre, el.filtres, el.vue, el.note, el.message);

    el.choix.addEventListener("change", () => afficher(el.choix.value));
    brancherSouris();
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape") window.javaBridge?.closeToolWindow?.();
    });
  }

  function message(texte) {
    el.message.textContent = texte || "";
    el.message.style.display = texte ? "block" : "none";
  }

  function appliquerTransform() {
    el.monde.style.transform = `translate(${etat.dx}px, ${etat.dy}px) scale(${etat.echelle})`;
    // Les lieux gardent la même taille à l'écran quel que soit le zoom.
    el.monde.style.setProperty("--inverse", String(1 / etat.echelle));
    // Trop dézoomé, les noms se chevauchent : on garde les points seuls.
    el.monde.classList.toggle("sansNoms", etat.echelle < SEUIL_NOMS);
  }

  function reinitialiserVue() {
    const c = etat.donnees?.maps?.[etat.id];
    if (!c) return;
    const r = el.vue.getBoundingClientRect();
    etat.echelle = Math.min(r.width, r.height) / c.pixelSize;
    etat.dx = (r.width - c.pixelSize * etat.echelle) / 2;
    etat.dy = (r.height - c.pixelSize * etat.echelle) / 2;
    appliquerTransform();
  }

  function brancherSouris() {
    el.vue.addEventListener("wheel", (e) => {
      e.preventDefault();
      const r = el.vue.getBoundingClientRect();
      const mx = e.clientX - r.left, my = e.clientY - r.top;
      const facteur = e.deltaY < 0 ? 1.15 : 1 / 1.15;
      const neuf = Math.min(2, Math.max(0.05, etat.echelle * facteur));
      const k = neuf / etat.echelle;
      etat.dx = mx - (mx - etat.dx) * k;
      etat.dy = my - (my - etat.dy) * k;
      etat.echelle = neuf;
      appliquerTransform();
    }, { passive: false });
    let prise = null;
    el.vue.addEventListener("mousedown", (e) => { prise = { x: e.clientX, y: e.clientY, dx: etat.dx, dy: etat.dy }; });
    window.addEventListener("mouseup", () => { prise = null; });
    window.addEventListener("mousemove", (e) => {
      if (!prise) return;
      etat.dx = prise.dx + e.clientX - prise.x;
      etat.dy = prise.dy + e.clientY - prise.y;
      appliquerTransform();
    });
    el.vue.addEventListener("dblclick", reinitialiserVue);
  }

  function appliquerFiltres() {
    for (const c of CALQUES) {
      const n = el.monde.querySelector(`[data-calque="${c.id}"]`);
      if (n) n.style.display = c.disponible && actif(c.id) ? "" : "none";
    }
  }

  function image(calque, chemin, taille) {
    const img = creer("img", "carteImage");
    img.dataset.calque = calque;
    img.draggable = false;
    img.style.width = img.style.height = taille + "px";
    img.addEventListener("error", () => img.remove());
    img.src = url(chemin);
    return img;
  }

  function afficher(id) {
    const c = etat.donnees.maps[id];
    if (!c) return;
    etat.id = id;
    etat.signatureObjets = null;
    ecrire(CLE_CARTE, id);
    el.monde.textContent = "";
    el.monde.style.width = el.monde.style.height = c.pixelSize + "px";

    el.monde.append(image("fond", c.background, c.pixelSize));
    if (c.paths) el.monde.append(image("chemins", c.paths, c.pixelSize));

    const lieux = creer("div", "carteCalque");
    lieux.dataset.calque = "lieux";
    let places = 0;
    for (const p of c.poi || []) {
      const pos = projeter(c, p.x, p.y);
      if (!pos) break;
      const pt = creer("div", "carteLieu");
      pt.style.left = pos.x + "px";
      pt.style.top = pos.y + "px";
      pt.append(creer("span", "carteLieuNom", p.name));
      lieux.append(pt);
      places++;
    }
    el.monde.append(lieux);
    for (const [id, classe] of [["decor", "carteObjetDecor"], ["ephemeres", "carteObjetEphemere"]]) {
      const calque = creer("div", "carteCalque");
      calque.dataset.calque = id;
      calque.dataset.classe = classe;
      el.monde.append(calque);
    }
    rafraichirObjets();
    appliquerFiltres();
    message(c.world ? "" : "Cette carte n'a pas de repère de position : le fond seul est affiché.");
    reinitialiserVue();
    void places;
  }

  // Redessine les objets vus. Ils sont rangés « carte non identifiée » : le meter
  // ne sait pas encore sur quelle carte on se trouve, ils s'affichent donc sur la
  // carte choisie.
  async function rafraichirObjets() {
    // Fenêtre cachée ou carte absente : rien à redessiner.
    if (document.hidden) return;
    const idCourant = etat.id;
    const c = etat.donnees?.maps?.[idCourant];
    if (!c || !c.world) { el.note.textContent = ""; return; }
    let objets = [];
    try { objets = (await window.javaBridge.getObjetsVus()) || []; } catch {}
    // La carte a changé pendant l'appel : ces objets seront redessinés au tour suivant.
    if (idCourant !== etat.id) return;
    const signature = [idCourant, voir("jeu"), voir("bruit"), !!etat.envobjs, JSON.stringify(objets)].join(":");
    if (signature === etat.signatureObjets) return;
    etat.signatureObjets = signature;
    // Les positions du jeu sont en centimètres ; `projeter` attend des mètres.
    const unite = etat.donnees.unitFactor || CM_PAR_M;
    const carte = { ...c, unitFactor: unite };
    let affiches = 0, masquesJeu = 0, masquesBruit = 0;
    for (const id of ["decor", "ephemeres"]) {
      const calque = el.monde.querySelector(`[data-calque="${id}"]`);
      if (!calque) continue;
      calque.textContent = "";
      const genre = id === "decor" ? "decor" : "ephemere";
      for (const o of objets.filter((o) => o.genre === genre)) {
        const info = genre === "decor" ? classer(etat.envobjs, o.modele) : null;
        if (info && !visible(info)) {
          if (info.surCarteDuJeu && !voir("jeu")) masquesJeu++;
          else masquesBruit++;
          continue;
        }
        const pos = projeter(carte, o.x / unite, o.y / unite);
        if (!pos) continue;
        const recolte = info && info.usage === "GatherSource";
        const pt = creer("div", "carteObjet " + calque.dataset.classe + (recolte ? " carteObjetRecolte" : ""));
        pt.style.left = pos.x + "px";
        pt.style.top = pos.y + "px";
        pt.title = info
          ? `${info.nom}${info.usage ? " (" + info.usage + ")" : ""} · vu ${o.fois_vu} fois`
          : "Objet éphémère";
        calque.append(pt);
        affiches++;
      }
    }
    const masques = masquesJeu + masquesBruit;
    el.note.textContent = objets.length
      ? `${affiches} objet(s) affiché(s), ${masques} masqué(s) (carte du jeu : ${masquesJeu}, bruit : ${masquesBruit})`
        + (etat.envobjs ? "" : " — noms indisponibles (données non téléchargées)")
        + " — carte du jeu non identifiée : affichés sur la carte choisie."
      : "";
  }

  async function demarrer() {
    construire();
    let e = null;
    try { e = await window.javaBridge.getFondsEtat(); } catch {}
    if (e) {
      el.statut.textContent = (e.version ? `fonds ${e.version} — ` : "") + e.dernier_bilan;
    }
    if (!e || !e.cache) {
      message("Fonds non téléchargés : activez le partage Guilde XIII NRV (onglet du meter). Le téléchargement se fait au plus une fois par jour.");
      return;
    }
    try {
      const r = await fetch(url("carte.json"));
      if (!r.ok) throw new Error(String(r.status));
      etat.donnees = await r.json();
    } catch (err) {
      message("Fonds illisibles (" + err.message + "). Ils seront retéléchargés à la prochaine synchronisation.");
      return;
    }
    // Référentiel des objets (noms, présence sur la carte du jeu) : facultatif.
    try {
      const r = await fetch(BASE + "envobjs.json");
      if (r.ok) etat.envobjs = (await r.json()).envobjs || null;
    } catch {}
    const ordre = Object.keys(CAMPS);
    const cartes = Object.entries(etat.donnees.maps);
    for (const camp of ordre) {
      const groupe = document.createElement("optgroup");
      groupe.label = CAMPS[camp];
      for (const [id, c] of cartes.filter(([, c]) => c.camp === camp)) {
        const o = document.createElement("option");
        o.value = id;
        o.textContent = c.name || id;
        groupe.append(o);
      }
      if (groupe.children.length) el.choix.append(groupe);
    }
    const derniere = lire(CLE_CARTE);
    const depart = derniere && etat.donnees.maps[derniere] ? derniere : (etat.donnees.maps.World_D_A ? "World_D_A" : cartes[0][0]);
    el.choix.value = depart;
    afficher(depart);
    setInterval(rafraichirObjets, 5000);
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", demarrer, { once: true });
  else demarrer();
})(typeof module !== "undefined" ? module : null);
