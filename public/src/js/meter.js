const createMeterUI = ({
  elList,
  dpsFormatter,
  getUserName,
  onClickUserRow,
  onHoverUserRow,
  onLeaveUserRow,
  getMetric,
  getMetricSecondaire,
  getSortDirection,
  getPinUserToTop,
  getPlayerLimit,
}) => {
  const MAX_CACHE = 32;
  const cjkRegex = /[\u3400-\u9FFF\uF900-\uFAFF]/;
  const classIconSrcByJob = new Map();

  const rowViewById = new Map();
  let lastVisibleIds = new Set();
  let pendingRenderRows = null;
  let renderRowsRafId = 0;

  const nowMs = () => (typeof performance !== "undefined" ? performance.now() : Date.now());

  const createRowView = (id) => {
    const rowEl = document.createElement("div");
    rowEl.className = "item";
    rowEl.style.display = "none";
    rowEl.dataset.rowId = String(id);

    // The bar spans the full row, so 100% reads as 100%. The readout keeps
    // its own scrim (see .dps in styles.css) for where the bar passes behind.
    const fillTrackEl = document.createElement("div");
    fillTrackEl.className = "fillTrack";

    const fillEl = document.createElement("div");
    fillEl.className = "fill";
    fillTrackEl.appendChild(fillEl);

    const contentEl = document.createElement("div");
    contentEl.className = "content";

    // Ladder position by damage. Kept separate from row order so that
    // "pin me to top" moves the row without misreporting the rank.
    const rankEl = document.createElement("span");
    rankEl.className = "rank";

    const classIconEl = document.createElement("div");
    classIconEl.className = "classIcon";

    const classIconImg = document.createElement("img");
    classIconImg.className = "classIconImg";
    classIconImg.style.visibility = "hidden";

    classIconImg.draggable = false;

    classIconEl.appendChild(classIconImg);

    const nameEl = document.createElement("div");
    nameEl.className = "name";

    // Supporter badge. Sits between the name and the combat power, hidden
    // unless the roster names this player. Inline SVG rather than a lucide
    // `data-lucide` element because rows are built and rebuilt constantly and
    // lucide only swaps placeholders when createIcons() runs.
    const supporterBadgeEl = document.createElement("span");
    supporterBadgeEl.className = "supporterBadge";
    supporterBadgeEl.style.display = "none";
    supporterBadgeEl.setAttribute("aria-hidden", "true");
    supporterBadgeEl.innerHTML =
      '<svg viewBox="0 0 24 24" width="12" height="12" fill="currentColor" ' +
      'aria-hidden="true"><path d="M3 7l4.2 3L12 4l4.8 6L21 7l-1.8 10H4.8L3 7z"/>' +
      '</svg>';

    // Party combat power, shown beside the name. Hidden unless the party roster
    // packet supplied a value for this player.
    const combatPowerEl = document.createElement("span");
    combatPowerEl.className = "combatPower";
    combatPowerEl.style.display = "none";

    // Ajout XIII NRV : le nombre de morts, a cote du pseudo. Un meter tiers
    // l'affiche ainsi et barbaxou l'a demande. Masque tant que le joueur n'est
    // pas tombe : une colonne de zeros n'apprend rien.
    const mortsEl = document.createElement("span");
    mortsEl.className = "morts";
    mortsEl.style.display = "none";
    // La tete de mort est dessinee, pas ecrite. Le caractere Unicode passait
    // par un echappement CSS (`content: "²0"`) que le minificateur lisait
    // comme `²` suivi d'un `0` : l'overlay affichait « ²0 2 » au lieu de
    // « 2 ». Et meme bien echappe, le glyphe depend d'une police qui peut ne
    // pas l'avoir. Un SVG en clair ne depend de rien — c'est deja la raison
    // donnee pour le badge de soutien, quelques lignes plus haut.
    const mortsIcone = document.createElement("span");
    mortsIcone.className = "mortsIcone";
    mortsIcone.setAttribute("aria-hidden", "true");
    mortsIcone.innerHTML =
      '<svg viewBox="0 0 16 16" width="9" height="9" fill="currentColor" ' +
      'aria-hidden="true"><path d="M8 1C4.7 1 2 3.5 2 6.6c0 1.9 1 3.6 2.6 4.6v1.6c0 ' +
      '.7.6 1.2 1.3 1.2h4.2c.7 0 1.3-.5 1.3-1.2v-1.6C13 10.2 14 8.5 14 6.6 14 3.5 ' +
      '11.3 1 8 1zm-2.3 5.2a1.3 1.3 0 1 1 0 2.6 1.3 1.3 0 0 1 0-2.6zm4.6 0a1.3 1.3 ' +
      '0 1 1 0 2.6 1.3 1.3 0 0 1 0-2.6zM8 9.4l.8 1.5H7.2L8 9.4z"/></svg>';
    const mortsNombre = document.createElement("span");
    mortsNombre.className = "mortsNombre";
    mortsEl.appendChild(mortsIcone);
    mortsEl.appendChild(mortsNombre);

    const dpsContainer = document.createElement("div");
    const dpsNumber = document.createElement("p");
    dpsContainer.className = "dps";
    const dpsContribution = document.createElement("p");
    dpsContribution.className = "dpsContribution";

    dpsContainer.appendChild(dpsNumber);
    dpsContainer.appendChild(dpsContribution);

    contentEl.appendChild(rankEl);
    contentEl.appendChild(classIconEl);
    contentEl.appendChild(nameEl);
    contentEl.appendChild(supporterBadgeEl);
    contentEl.appendChild(combatPowerEl);
    contentEl.appendChild(mortsEl);
    contentEl.appendChild(dpsContainer);
    rowEl.appendChild(fillTrackEl);
    rowEl.appendChild(contentEl);

    const view = {
      id,
      rowEl,
      prevContribClass: "",
      nameEl,
      supporterBadgeEl,
      combatPowerEl,
      mortsEl,
      mortsNombreEl: mortsNombre,
      rankEl,
      dpsContainer,
      classIconEl,
      classIconImg,
      dpsNumber,
      dpsContribution,
      fillEl,
      currentRow: null,
      lastSeenAt: 0,
      isVisible: false,
      lastNameText: "",
      lastCombatPowerText: "",
      lastMortsText: "",
      lastIsCjk: false,
      lastIsSupporter: false,
      lastMetricText: "",
      lastContributionText: "",
      lastRankText: "",
      lastFillRatio: -1,
      lastClassIconSrc: "",
      lastIsUser: false,
      lastIsIdentifying: false,
      hoverRipplePlayed: false,
      hoverRippleTimer: null,
    };

    rowEl.addEventListener("mouseenter", (event) => {
      if (!view.hoverRipplePlayed) {
        view.rowEl.classList.add("hoverRippleOnce");
        view.hoverRipplePlayed = true;
        if (view.hoverRippleTimer) {
          clearTimeout(view.hoverRippleTimer);
        }
        view.hoverRippleTimer = setTimeout(() => {
          view.rowEl.classList.remove("hoverRippleOnce");
          view.hoverRippleTimer = null;
        }, 950);
      }
      onHoverUserRow?.(view.currentRow, event);
    });


    rowEl.addEventListener("mousemove", (event) => {
      if (view._lastMoveMs && nowMs() - view._lastMoveMs < 100) return;
      view._lastMoveMs = nowMs();
      onHoverUserRow?.(view.currentRow, event);
    });
    rowEl.addEventListener("mouseleave", () => {
      view.hoverRipplePlayed = false;
      if (view.hoverRippleTimer) {
        clearTimeout(view.hoverRippleTimer);
        view.hoverRippleTimer = null;
      }
      view.rowEl.classList.remove("hoverRippleOnce");
      onLeaveUserRow?.(view.currentRow);
    });

    rowEl.addEventListener("click", () => {
      // if (view.currentRow?.isUser)
      onClickUserRow?.(view.currentRow);
    });

    return view;
  };

  const getRowView = (id) => {
    let view = rowViewById.get(id);
    if (!view) {
      view = createRowView(id);
      rowViewById.set(id, view);
      elList.appendChild(view.rowEl);
    }
    return view;
  };

  const getDisplayRows = (sortedAll) => {
    const limit = (typeof getPlayerLimit === "function" ? getPlayerLimit() : 6) || 6;
    const topN = sortedAll.slice(0, limit);
    const user = sortedAll.find((x) => x.isUser);

    if (!user) return topN;
    const pinUser = typeof getPinUserToTop === "function" && getPinUserToTop();
    if (pinUser) {
      return [user, ...topN.filter((row) => !row.isUser)];
    }
    if (topN.some((x) => x.isUser)) return topN;
    return [...topN, user];
  };

  const pruneCache = (keepIds) => {
    if (rowViewById.size <= MAX_CACHE) return;

    const candidates = [];
    for (const [id, view] of rowViewById) {
      if (keepIds.has(id)) {
        continue;
      }
      candidates.push({ id, t: view.lastSeenAt || 0 });
    }

    candidates.sort((a, b) => a.t - b.t); // 오래된거 제거

    for (let i = 0; rowViewById.size > MAX_CACHE && i < candidates.length; i++) {
      const id = candidates[i].id;
      const view = rowViewById.get(id);
      if (!view) continue;
      view.rowEl.remove();
      rowViewById.delete(id);
    }
  };

  const resolveMetric = (row) => {
    if (typeof getMetric === "function") {
      return getMetric(row);
    }
    const dps = Number(row?.dps) || 0;
    const suffix = window.i18n?.t?.("meter.dpsSuffix", "/s") ?? "/s";
    return { value: dps, text: `${dpsFormatter.format(dps)}${suffix}` };
  };

  let lastOrderKey = "";

  const renderRows = (rows, rankById) => {
    const now = nowMs();
    const nextVisibleIds = new Set();

    const hadRows = elList.classList.contains("hasRows");
    const hasRows = rows.length > 0;
    if (hadRows !== hasRows) {
      elList.classList.toggle("hasRows", hasRows);
    }

    let topMetric = 1;
    for (const row of rows) {
      const metricValue = Number(resolveMetric(row)?.value) || 0;
      if (metricValue > topMetric) topMetric = metricValue;
    }
    const visibleTotalDamage = rows.reduce((sum, row) => sum + (Number(row?.totalDamage) || 0), 0);

    // Build order key to detect if DOM reordering is needed
    let orderKey = "";
    const validRows = [];
    for (const row of rows) {
      if (!row) continue;
      const id = row.id ?? row.name;
      if (!id) continue;
      validRows.push({ row, id });
      orderKey += id + ",";
    }

    const needsReorder = orderKey !== lastOrderKey;
    lastOrderKey = orderKey;

    for (const { row, id } of validRows) {
      nextVisibleIds.add(id);

      const view = getRowView(id);
      view.currentRow = row;
      view.lastSeenAt = now;

      if (!view.isVisible) {
        view.rowEl.style.display = "";
        view.isVisible = true;
      }

      const isUser = !!row.isUser;
      if (view.lastIsUser !== isUser) {
        view.rowEl.classList.toggle("isUser", isUser);
        view.lastIsUser = isUser;
      }

      const isIdentifying = !!row.isIdentifying;
      if (view.lastIsIdentifying !== isIdentifying) {
        view.rowEl.classList.toggle("isIdentifying", isIdentifying);
        view.lastIsIdentifying = isIdentifying;
      }

      const rowId = row.id ?? row.name ?? "";
      const nameText = row.isIdentifying
        ? window.i18n?.format?.("meter.identifyingPlayer", { id: rowId }, `#${rowId}`) ??
          `#${rowId}`
        : row.name ?? "";
      if (view.lastNameText !== nameText) {
        view.nameEl.textContent = nameText;
        view.lastNameText = nameText;
      }

      const isCjk = cjkRegex.test(nameText);
      // Its own cached field rather than folded into the name: lastNameText
      // short-circuits when the text is unchanged, so a supporter whose name
      // stays the same would never get the class applied.
      const isSupporter = !!row.isSupporter;
      if (view.lastIsSupporter !== isSupporter) {
        view.nameEl.classList.toggle("isSupporter", isSupporter);
        view.supporterBadgeEl.style.display = isSupporter ? "inline-flex" : "none";
        view.lastIsSupporter = isSupporter;
      }

      if (view.lastIsCjk !== isCjk) {
        view.nameEl.classList.toggle("isCjk", isCjk);
        view.lastIsCjk = isCjk;
      }

      // Combat power sits beside the name, abbreviated to thousands the way
      // players quote it — 889,100 reads as "889k", 889,545 as "890k". The
      // exact figure stays on the row for the details panel. Below 500 the
      // abbreviation would collapse to "0k", so show the raw number there.
      // Ajout XIII NRV : le Combat Power ne s'affiche plus a cote du nom pendant
      // le combat. barbaxou l'a demande : la ligne fait trente pixels, et ce
      // chiffre ne change pas d'un combat a l'autre — il encombrait sans
      // informer. Il reste lu et envoye au site, ou il a sa place.
      // Ajout XIII NRV : le Combat Power revient a cote du pseudo. Cette
      // ligne valait la chaine vide depuis qu'il avait ete retire de
      // l'overlay ; barbaxou le veut de nouveau, comme un meter tiers le
      // montre. Zero veut dire inconnu — il ne vient que du paquet de groupe —
      // donc on n'affiche rien plutot qu'un zero trompeur.
      // Abrégé : « 95 000 » prenait toute la place à côté du pseudo. Deux
      // décimales, comme un meter tiers l'affiche (« CP : 93,58K »).
      const cp = Math.trunc(Number(row?.combatPower)) || 0;
      const combatPowerText =
        cp >= 1000
          ? `${(cp / 1000).toFixed(2).replace(".", ",")}K`
          : cp > 0
            ? String(cp)
            : "";
      if (view.lastCombatPowerText !== combatPowerText) {
        view.combatPowerEl.textContent = combatPowerText;
        view.combatPowerEl.style.display = combatPowerText ? "" : "none";
        view.lastCombatPowerText = combatPowerText;
      }

      const morts = Math.trunc(Number(row?.deaths)) || 0;
      const mortsText = morts > 0 ? String(morts) : "";
      if (view.lastMortsText !== mortsText) {
        view.mortsNombreEl.textContent = mortsText;
        view.mortsEl.style.display = mortsText ? "" : "none";
        view.mortsEl.title = mortsText ? `${morts} mort${morts > 1 ? "s" : ""}` : "";
        view.lastMortsText = mortsText;
      }

      if (row.job && !!row.job) {
        if (!classIconSrcByJob.has(row.job)) {
          classIconSrcByJob.set(row.job, `./assets/${row.job}.png`);
        }
        const src = classIconSrcByJob.get(row.job);
        if (view.lastClassIconSrc !== src) {
          // Les images sont recopiées à la racine dans l'application installée,
          // mais restent sous src/ pendant le développement : si la première
          // adresse échoue, on essaie l'autre une fois.
          //
          // Et si la seconde échoue aussi, on cache l'image au lieu de laisser
          // l'icône cassée du navigateur. Ce n'est pas théorique : tant que la
          // classe n'est pas connue — au lancement, avant le premier combat —
          // elle vaut « Unknown », et aucun fichier ne porte ce nom. barbaxou l'a
          // vu le 05/10/2026 sur la première ligne affichée.
          view.classIconImg.onerror = () => {
            view.classIconImg.onerror = () => {
              view.classIconImg.onerror = null;
              view.classIconImg.removeAttribute("src");
              view.classIconImg.style.visibility = "hidden";
              view.lastClassIconSrc = "";
            };
            view.classIconImg.src = `./src/assets/${row.job}.png`;
          };
          view.classIconImg.src = src;
          view.lastClassIconSrc = src;
        }
        if (view.classIconImg.style.visibility !== "visible") {
          view.classIconImg.style.visibility = "visible";
        }
      } else {
        if (view.lastClassIconSrc) {
          view.lastClassIconSrc = "";
          view.classIconImg.removeAttribute("src");
        }
        if (view.classIconImg.style.visibility !== "hidden") {
          view.classIconImg.style.visibility = "hidden";
        }
      }

      const metric = resolveMetric(row) || { value: 0, text: "-" };
      const metricValue = Number(metric.value) || 0;
      const damageContribution =
        visibleTotalDamage > 0 ? (Number(row.totalDamage) / visibleTotalDamage) * 100 : 0;

      let contributionClass = "";
      if (damageContribution < 3) {
        contributionClass = "error";
      } else if (damageContribution < 5) {
        contributionClass = "warning";
      }
      if (view.prevContribClass !== contributionClass) {
        if (view.prevContribClass) {
          view.rowEl.classList.remove(view.prevContribClass);
        }
        if (contributionClass) {
          view.rowEl.classList.add(contributionClass);
        }
        view.prevContribClass = contributionClass;
      }

      const metricText = metric.text;
      if (view.lastMetricText !== metricText) {
        view.dpsNumber.textContent = metricText;
        view.lastMetricText = metricText;
      }

      // Ajout XIII NRV : cette zone montrait toujours la part des degats. Elle
      // montre maintenant la mesure choisie, ou rien si l'on n'en veut pas.
      const secondaire =
        typeof getMetricSecondaire === "function" ? getMetricSecondaire(row) : null;
      const contributionText = secondaire
        ? secondaire.text
        : `${damageContribution.toFixed(1)}%`;
      if (view.lastContributionText !== contributionText) {
        view.dpsContribution.textContent = contributionText;
        view.lastContributionText = contributionText;
      }

      const rankText = String(rankById?.get(id) ?? "");
      if (view.lastRankText !== rankText) {
        view.rankEl.textContent = rankText;
        view.rowEl.classList.toggle("isRankOne", rankText === "1");
        view.lastRankText = rankText;
      }

      const ratio = Math.max(0, Math.min(1, metricValue / topMetric));
      if (view.lastFillRatio !== ratio) {
        // Width rather than scaleX: the bar's lit leading edge is a fixed
        // 2px child, and a scaled parent would squash it (and its glow) by
        // the fill ratio. Six absolutely-positioned rows, rAF-throttled, so
        // the layout cost is not measurable.
        view.fillEl.style.width = `${(ratio * 100).toFixed(2)}%`;
        view.lastFillRatio = ratio;
      }

      // Only touch DOM order when the sorted list actually changed
      if (needsReorder) {
        elList.appendChild(view.rowEl);
      }
    }

    for (const id of lastVisibleIds) {
      if (nextVisibleIds.has(id)) continue;
      const view = rowViewById.get(id);
      if (view && view.isVisible) {
        view.rowEl.style.display = "none";
        view.isVisible = false;
      }
    }

    lastVisibleIds = nextVisibleIds;

    pruneCache(nextVisibleIds);
  };

  const flushPendingRows = () => {
    renderRowsRafId = 0;
    const rows = pendingRenderRows;
    pendingRenderRows = null;
    if (!rows) return;

    const arr = Array.isArray(rows) ? rows.slice() : [];
    const sortDirection = typeof getSortDirection === "function" ? getSortDirection() : "desc";
    arr.sort((a, b) => {
      const aMetric = Number(resolveMetric(a)?.value) || 0;
      const bMetric = Number(resolveMetric(b)?.value) || 0;
      return sortDirection === "asc" ? aMetric - bMetric : bMetric - aMetric;
    });

    // Rank is always "1 = most damage", independent of the ascending /
    // descending list toggle and of pinning the user to the top.
    const rankById = new Map();
    arr
      .slice()
      .sort((a, b) => (Number(resolveMetric(b)?.value) || 0) - (Number(resolveMetric(a)?.value) || 0))
      .forEach((row, index) => {
        const id = row?.id ?? row?.name;
        if (id) rankById.set(id, index + 1);
      });

    renderRows(getDisplayRows(arr), rankById);
  };

  const updateFromRows = (rows) => {
    pendingRenderRows = rows;
    if (renderRowsRafId) return;
    renderRowsRafId = requestAnimationFrame(flushPendingRows);
  };
  const onResetMeterUi = () => {
    if (renderRowsRafId) {
      cancelAnimationFrame(renderRowsRafId);
      renderRowsRafId = 0;
    }
    pendingRenderRows = null;
    lastOrderKey = "";
    elList.classList.remove("hasRows");
    lastVisibleIds = new Set();

    const battleTimeEl = elList.querySelector(".battleTime");
    if (battleTimeEl) {
      elList.replaceChildren(battleTimeEl);
    } else {
      elList.replaceChildren();
    }

    rowViewById.clear();
    classIconSrcByJob.clear();
  };

  const getRowById = (id) => {
    if (id === null || id === undefined) return null;
    const view = rowViewById.get(id) || rowViewById.get(String(id));
    return view?.currentRow || null;
  };

  return { updateFromRows, onResetMeterUi, getRowById };
};
