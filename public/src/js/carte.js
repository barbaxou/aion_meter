// Fenêtre de la carte (ajout XIII NRV).
//
// Lit le cache des fonds de carte (`fonds/carte/…`, téléchargé depuis le site)
// par le protocole `fonds://`. Aucune dépendance : zoom et déplacement sont
// écrits ici. Les calques de fond sont décrits dans CALQUES ; les objets vus en
// jouant se filtrent par catégorie (voir `carteCategories.js`).
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
  ];

  // Échelle (écran / pixels de la carte) sous laquelle les noms sont masqués.
  const SEUIL_NOMS = 0.3;
  // Même chose pour les noms des objets vus, plus nombreux : il faut zoomer davantage.
  const SEUIL_NOMS_OBJETS = 0.5;

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

  // Un objet est « confirmé actif » quand le meter vient de le voir : un éphémère
  // présent, ou un objet vu dans les dernières minutes. Passé ce délai on ne sait plus
  // s'il est toujours là (le jeu n'annonce pas toutes les disparitions) : pas de cercle.
  const CONFIRME_MS = 5 * 60 * 1000;
  function confirme(objet, maintenant) {
    if (objet.genre === "ephemere") return true;
    return typeof objet.vu_a === "number" && maintenant - objet.vu_a <= CONFIRME_MS;
  }

  const CAMPS = { asmodien: "Asmodiens", elyos: "Élyséens", abysse: "Abysses" };

  if (racine && typeof module !== "undefined" && module.exports) {
    module.exports = { projeter, CALQUES, classer, confirme };
    return;
  }
  if (typeof window === "undefined" || window.A2_VIEW !== "carte") {
    window.Carte = { projeter, CALQUES, classer, confirme };
    return;
  }

  // ---- Vue ------------------------------------------------------------------

  if (!window.CarteCategories) {
    // Ne devrait pas arriver (balise script dans index.html) : on le dit plutôt que de laisser une fenêtre vide.
    document.addEventListener("DOMContentLoaded", () => {
      const panneau = document.querySelector(".cartePanel");
      if (panneau) panneau.textContent = "Catégories d'objets introuvables (carteCategories.js non chargé).";
    });
    return;
  }
  const { CATEGORIES, categoriser } = window.CarteCategories;
  const CAT = Object.fromEntries(CATEGORIES.map((c) => [c.id, c]));
  const SVG = "http://www.w3.org/2000/svg";

  const BASE = /Windows/i.test(navigator.userAgent) ? "http://fonds.localhost/" : "fonds://localhost/";
  const url = (chemin) => BASE + "carte/" + chemin;
  const CLE_FILTRES = "carte.filtres";
  const CLE_CARTE = "carte.derniere";

  const lire = (cle) => { try { return window.localStorage.getItem(cle); } catch { return null; } };
  const ecrire = (cle, v) => { try { window.localStorage.setItem(cle, v); } catch {} };

  let filtres = {};
  try { filtres = JSON.parse(lire(CLE_FILTRES) || "{}") || {}; } catch { filtres = {}; }
  const actif = (id) => filtres[id] !== false;
  // Une catégorie est affichée tant qu'on ne l'a pas décochée, sauf « Autres » (repères
  // sans intérêt comme les pianos ou les points d'arrivée), cachée tant qu'on ne la coche pas.
  const voirCat = (id) => (filtres["cat_" + id] === undefined ? id !== "autre" : filtres["cat_" + id]);
  const voirNoms = () => filtres.noms !== false;

  const etat = { donnees: null, envobjs: null, id: null, echelle: 1, dx: 0, dy: 0 };
  const el = {};

  function creer(tag, classe, texte) {
    const n = document.createElement(tag);
    if (classe) n.className = classe;
    if (texte != null) n.textContent = texte;
    return n;
  }

  // Pictogramme au trait, construit sans `innerHTML`.
  function pictogramme(trace) {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("width", "14");
    svg.setAttribute("height", "14");
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", trace);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", "#fff");
    path.setAttribute("stroke-width", "2.2");
    path.setAttribute("stroke-linecap", "round");
    path.setAttribute("stroke-linejoin", "round");
    svg.append(path);
    return svg;
  }

  // « vu il y a 3 jours », pour l'infobulle et pour estomper les cubes anciens.
  function ilYa(ms) {
    const d = Date.now() - ms;
    if (!(d >= 0)) return "";
    if (d < 60000) return "vu à l'instant";
    if (d < 3600000) return `vu il y a ${Math.floor(d / 60000)} min`;
    if (d < 86400000) return `vu il y a ${Math.floor(d / 3600000)} h`;
    const j = Math.floor(d / 86400000);
    return `vu il y a ${j} jour${j > 1 ? "s" : ""}`;
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

    const noms = creer("label", "carteFiltre carteFiltreExtra");
    const caseNoms = creer("input");
    caseNoms.type = "checkbox";
    caseNoms.checked = voirNoms();
    caseNoms.addEventListener("change", () => {
      filtres.noms = caseNoms.checked;
      ecrire(CLE_FILTRES, JSON.stringify(filtres));
      appliquerTransform();
    });
    noms.append(caseNoms, document.createTextNode(" Noms"));
    el.filtres.append(noms);

    // Une case par catégorie d'objet, avec son pictogramme et son compteur.
    el.categories = creer("div", "carteFiltres carteCategories");
    el.compteurs = {};
    el.etiquettesCat = {};
    for (const c of CATEGORIES) {
      const label = creer("label", "carteFiltre carteCat vide");
      const case_ = creer("input");
      case_.type = "checkbox";
      case_.checked = voirCat(c.id);
      case_.addEventListener("change", () => {
        filtres["cat_" + c.id] = case_.checked;
        ecrire(CLE_FILTRES, JSON.stringify(filtres));
        rafraichirObjets();
      });
      const rond = creer("span", "carteCatRond");
      rond.style.background = c.couleur;
      rond.append(pictogramme(c.picto));
      const compteur = creer("span", "carteCatNombre", " (0)");
      label.append(case_, rond, document.createTextNode(" " + c.libelle), compteur);
      el.compteurs[c.id] = compteur;
      el.etiquettesCat[c.id] = label;
      el.categories.append(label);
    }

    el.vue = creer("div", "carteVue");
    el.monde = creer("div", "carteMonde");
    el.vue.append(el.monde);
    el.message = creer("div", "carteMessage");
    el.note = creer("div", "carteNote");
    panneau.append(el.barre, el.filtres, el.categories, el.vue, el.note, el.message);

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
    el.monde.classList.toggle("sansNomsObjets", !voirNoms() || etat.echelle < SEUIL_NOMS_OBJETS);
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
    const objets = creer("div", "carteCalque");
    objets.dataset.calque = "objets";
    el.monde.append(objets);
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
    await chargerReferentiel();
    let objets = [];
    try { objets = (await window.javaBridge.getObjetsVus()) || []; } catch {}
    // La carte a changé pendant l'appel : ces objets seront redessinés au tour suivant.
    if (idCourant !== etat.id) return;
    const signature = [
      idCourant, CATEGORIES.map((k) => (voirCat(k.id) ? 1 : 0)).join(""),
      !!etat.envobjs, Math.floor(Date.now() / 60000), JSON.stringify(objets),
    ].join(":");
    if (signature === etat.signatureObjets) return;
    etat.signatureObjets = signature;
    // Les positions du jeu sont en centimètres ; `projeter` attend des mètres.
    const unite = etat.donnees.unitFactor || CM_PAR_M;
    const carte = { ...c, unitFactor: unite };
    const calque = el.monde.querySelector('[data-calque="objets"]');
    if (!calque) return;
    calque.textContent = "";
    const compte = {};
    let affiches = 0, actifs = 0;
    const maintenant = Date.now();
    for (const o of objets) {
      const info = o.genre === "decor" ? classer(etat.envobjs, o.modele) : null;
      const cat = categoriser(etat.envobjs, o.modele, o.genre);
      compte[cat] = (compte[cat] || 0) + 1;
      if (!voirCat(cat)) continue;
      const pos = projeter(carte, o.x / unite, o.y / unite);
      if (!pos) continue;
      const actif = confirme(o, maintenant);
      calque.append(creerPastille(o, info, cat, pos, actif));
      affiches++;
      if (actif) actifs++;
    }
    for (const k of CATEGORIES) {
      el.compteurs[k.id].textContent = ` (${compte[k.id] || 0})`;
      el.etiquettesCat[k.id].classList.toggle("vide", !compte[k.id]);
    }
    el.note.textContent = objets.length
      ? `${affiches} objet(s) affiché(s), dont ${actifs} confirmé(s) actif(s) (cercle vert : vu à l'instant)`
        + (etat.envobjs ? "" : " — noms indisponibles (données non téléchargées)")
        + " — carte du jeu non identifiée : affichés sur la carte choisie."
      : "";
  }

  // Une pastille : disque coloré avec le pictogramme de sa catégorie, son nom à côté
  // (visible à partir d'un certain zoom) et le détail au survol.
  function creerPastille(o, info, cat, pos, actif) {
    const def = CAT[cat] || CAT.autre;
    const nom = info ? info.nom : (o.genre === "ephemere" ? "Âme (objet éphémère)" : "Monstre");
    const pt = creer("div", "carteObjet");
    pt.style.left = pos.x + "px";
    pt.style.top = pos.y + "px";
    const rond = creer("div", "carteObjetRond");
    rond.style.background = def.couleur;
    rond.append(pictogramme(def.picto));
    // Cercle vert : le meter vient de le voir. Sans cercle : vu plus tôt, non confirmé.
    if (actif) rond.classList.add("carteObjetActif");
    let detail = `${nom} — ${def.libelle}`;
    if (info && info.surCarteDuJeu) detail += " · déjà sur la carte du jeu";
    if (o.genre === "decor") detail += ` · vu ${o.fois_vu} fois`;
    const quand = ilYa(o.vu_a);
    if (quand) detail += ` · ${quand}`;
    detail += actif ? " · actif" : " · non confirmé";
    if (cat === "cube") {
      // Un cube n'est pas une zone : c'est l'endroit où il a été vu. On estompe les
      // anciens (il a pu être pris ou disparaître).
      const sombre = etat.envobjs?.[String(o.modele)]?.category === "HiddenCubeDark";
      if (sombre) rond.style.background = "#9b3f86";
      const jours = (Date.now() - o.vu_a) / 86400000;
      pt.style.opacity = jours <= 1 ? "1" : jours <= 7 ? "0.75" : "0.5";
      detail = `${nom} (${sombre ? "sombre" : "clair"}) · vu ici${o.fois_vu > 1 ? ` ${o.fois_vu} fois` : ""} · ${quand} · ${actif ? "actif" : "non confirmé"}`;
    }
    rond.title = detail;
    pt.append(rond, creer("span", "carteObjetNom", nom));
    return pt;
  }

  // Référentiel des objets (noms, présence sur la carte du jeu) : facultatif, et
  // redemandé tant qu'il manque, car il peut arriver pendant que la fenêtre est ouverte.
  async function chargerReferentiel() {
    if (etat.envobjs) return;
    try {
      const r = await fetch(BASE + "envobjs.json");
      if (r.ok) etat.envobjs = (await r.json()).envobjs || null;
    } catch {}
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
    await chargerReferentiel();
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
