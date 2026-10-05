//! Les dégâts subis arrivent-ils jusqu'au stockage, et sans rien coûter au reste ?
//!
//! Les coups reçus ne voyagent pas dans un paquet à eux : ils sont dans les
//! mêmes paquets `04 38` que les coups donnés, sous une compétence à sept
//! chiffres — la plage des compétences de monstres. `parsing_damage_inner`
//! s'arrêtait net devant cette plage, alors que `append_damage` l'attend pour
//! compter un dégât reçu : l'un jetait ce que l'autre guettait.
//!
//! Ce test rejoue un journal réel et vérifie les deux choses qui comptent :
//! que les coups reçus arrivent, et que **les coups donnés ne changent pas**.
//! La seconde est la plus importante : c'est elle qui protège les chiffres déjà
//! validés contre l'analyseur du jeu (05/10/2026, deux combats, 0,4 et 0,8 %
//! d'écart).
//!
//! Le journal vit hors du dépôt — il porte le trafic d'un compte. Sans lui, le
//! test se signale et passe.
//!
//!     cargo test --test xiiinrv_degats_subis -- --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::stream_assembler::StreamAssembler;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::combat::dps_calculator::DpsCalculator;
use xiiinrv_meter_lib::combat::ping_tracker::PingTracker;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};

const JOURNAL: &str = r"C:\Users\Baba\AppData\Local\Temp\claude\D--6---dev-2---Claude-code\88ca9fb3-1c53-44c3-b00d-d2ffdcc5b60c\scratchpad\session_1110.txt";
const NPCS: &str = "../src/data/i18n/npcs/fr.json";

/// Ce que l'analyseur de combat du jeu affichait, relevé à l'écran par barbaxou
/// le 05/10/2026 sur le serveur TW.
struct Attendu {
    boss: &'static str,
    /// Dégâts donnés **par ce rejeu**, relevés avant la correction. Elle ne doit
    /// pas les changer d'une unité : c'est tout l'objet de ce test. Ce n'est pas
    /// le total du combat réel — voir l'avertissement en tête de fichier.
    donne_en_rejeu: i64,
    /// Ce que l'analyseur du jeu affichait, pour mémoire. Non vérifié ici,
    /// puisque le rejeu ne reproduit pas le combat entier.
    donne_au_jeu: i64,
    subi_au_jeu: i64,
    coups_subis_au_jeu: i32,
}

const ATTENDUS: [Attendu; 2] = [
    Attendu {
        boss: "Kwapo",
        donne_en_rejeu: 152_511,
        donne_au_jeu: 499_018,
        subi_au_jeu: 21_761,
        coups_subis_au_jeu: 9,
    },
    Attendu {
        boss: "Duanka",
        donne_en_rejeu: 173_372,
        donne_au_jeu: 525_184,
        subi_au_jeu: 10_740,
        coups_subis_au_jeu: 9,
    },
];

fn hex(h: &str) -> Option<Vec<u8>> {
    if !h.len().is_multiple_of(2) {
        return None;
    }
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok())
        .collect()
}

fn horodatage_ms(ts: &str) -> Option<i64> {
    let d: chrono::DateTime<chrono::FixedOffset> = ts.parse().ok()?;
    Some(d.timestamp_millis())
}

/// Un combat tel que le meter l'enregistre.
#[derive(Debug, Clone)]
struct Combat {
    boss: String,
    inflige: i64,
    recu: i64,
    coups_recus: i32,
}

/// Rejoue le journal comme le répartiteur le fait en direct, et relève les
/// combats **un par un, au moment où ils se terminent** — et non l'état du
/// stockage à la fin, qui n'est qu'un reste une fois la cible changée. C'est
/// l'erreur qui m'avait fait conclure à tort, le 05/10 au matin, que le journal
/// ne contenait que 30 % du trafic.
fn rejouer() -> Option<Vec<Combat>> {
    let texte = std::fs::read_to_string(JOURNAL).ok()?;

    let npc_lookup = Arc::new(NpcLookup::new());
    match std::fs::read_to_string(NPCS) {
        Ok(j) => npc_lookup.load_from_json(&j),
        Err(e) => panic!(
            "noms de monstres illisibles ({NPCS}) : {e} — sans eux aucun combat \
             de boss n'est relevé et le test ne prouverait rien"
        ),
    }

    let storage = Arc::new(DataStorage::new());
    let skills = Arc::new(SkillLookup::new());
    let mut processeur = StreamProcessor::new(storage.clone(), skills.clone(), npc_lookup.clone());
    let mut calcul = DpsCalculator::new(
        storage.clone(),
        skills,
        npc_lookup,
        Arc::new(PingTracker::new()),
    );

    let mut flux: HashMap<String, StreamAssembler> = HashMap::new();
    let mut combats: Vec<Combat> = Vec::new();

    for ligne in texte.lines() {
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let mut champs = ligne.splitn(3, '|');
        let (ts, cle, h) = match (champs.next(), champs.next(), champs.next()) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => continue,
        };
        let Some(octets) = hex(h.trim()) else { continue };

        // Épingler l'horloge sur l'heure de capture : `DataStorage` décide ses
        // remises à zéro contre « maintenant », et un journal rejoué en trois
        // secondes n'aurait plus rien à voir avec la session d'origine.
        if let Some(ms) = horodatage_ms(ts) {
            processeur.set_override_timestamp(Some(ms));
        }
        flux.entry(cle.to_string())
            .or_insert_with(StreamAssembler::new)
            .process_chunk(&octets, &mut processeur);

        relever(&mut calcul, &storage, &mut combats, false);
    }
    relever(&mut calcul, &storage, &mut combats, true);
    processeur.set_override_timestamp(None);
    Some(combats)
}

fn relever(
    calcul: &mut DpsCalculator,
    storage: &Arc<DataStorage>,
    combats: &mut Vec<Combat>,
    forcer: bool,
) {
    let releves = if forcer {
        calcul.snapshot_boss_fights_force()
    } else {
        calcul.snapshot_boss_fights()
    };
    for r in releves {
        // Le FightRecord ne porte pas les dégâts subis : on les lit sur la cible
        // correspondante, tant qu'elle est encore dans le stockage.
        let (mut recu, mut coups) = (0i64, 0i32);
        if let Some(td) = storage.get_combat_snapshot_light().get(&r.target_id) {
            for ad in td.actors.values() {
                recu += ad.damage_received;
                coups += ad.hits_received;
            }
        }
        combats.push(Combat {
            boss: r.boss_name.clone(),
            inflige: r.total_damage as i64,
            recu,
            coups_recus: coups,
        });
    }
}

#[test]
fn les_degats_subis_arrivent_sans_rien_changer_aux_degats_donnes() {
    let Some(combats) = rejouer() else {
        eprintln!("journal absent, test ignoré : {JOURNAL}");
        return;
    };
    assert!(
        !combats.is_empty(),
        "aucun combat relevé : les noms de monstres sont-ils chargés ?"
    );

    println!("{} relevés, le plus complet par boss :", combats.len());
    let mut vus: std::collections::BTreeMap<String, &Combat> = Default::default();
    for c in &combats {
        let e = vus.entry(c.boss.clone()).or_insert(c);
        if c.inflige > e.inflige {
            *e = c;
        }
    }
    for (_nom, c) in &vus {
        println!(
            "   {:<30} donné {:>8}   subi {:>7} sur {} coups",
            c.boss, c.inflige, c.recu, c.coups_recus
        );
    }

    // Au moins un combat doit remonter des coups reçus : sans cela la
    // correction ne prend pas du tout, et le test n'aurait rien attrapé.
    let mut au_moins_un_subi = false;

    for attendu in &ATTENDUS {
        // Un boss en cours est re-relevé toutes les trente secondes : le bon
        // relevé est le plus complet, celui que le meter garde en écrasant les
        // précédents.
        let Some(c) = combats
            .iter()
            .filter(|c| c.boss.contains(attendu.boss))
            .max_by_key(|c| c.inflige)
        else {
            eprintln!("\n« {} » absent du journal, comparaison ignorée", attendu.boss);
            continue;
        };
        println!("\n=== {} ===", c.boss);

        // 1. L'assertion qui protège l'acquis : lire les coups reçus ne doit
        //    rien changer aux coups donnés. Au chiffre près, pas « à peu près ».
        println!(
            "donné : {} en rejeu (référence {}) — pour mémoire, {} au jeu",
            c.inflige, attendu.donne_en_rejeu, attendu.donne_au_jeu
        );
        assert_eq!(
            c.inflige, attendu.donne_en_rejeu,
            "{} : les dégâts donnés ont changé ({} au lieu de {}). Lire les coups \
             reçus ne doit toucher à rien d'autre — c'est la seule chose que ce \
             test protège vraiment.",
            c.boss, c.inflige, attendu.donne_en_rejeu
        );

        // 2. Ce que la correction apporte. On note, on n'exige pas combat par
        //    combat : sur ce journal, un combat en remonte et l'autre non, et
        //    tant que le rejeu ne reproduit pas un combat entier, exiger un
        //    chiffre ici serait feindre une garantie qu'on n'a pas.
        println!(
            "subi  : {} sur {} coups — au jeu : {} sur {}",
            c.recu, c.coups_recus, attendu.subi_au_jeu, attendu.coups_subis_au_jeu
        );
        if c.recu > 0 {
            au_moins_un_subi = true;
        }
    }

    assert!(
        au_moins_un_subi,
        "aucun combat ne remonte de coup reçu : la correction du `break` sur les          compétences de monstres ne prend pas. Voir l'en-tête de ce fichier."
    );
}
