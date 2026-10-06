//! Rejoue une capture dans le **vrai** chemin de lecture et dit ce que la fiche
//! en retire.
//!
//! Le 06/10/2026, la capture de barbaxou contenait `33 36` (la fiche) et
//! `00 90` (les pets) à 00:29:24 — donc ces paquets avaient franchi tous les
//! filtres du dispatcher, puisque c'est lui qui écrit ce fichier. Et pourtant le
//! module de collecte n'a rien journalisé de la session, et son écran annonçait
//! les pets, les PV et les PM manquants.
//!
//! Deux explications possibles, et il faut les départager :
//!
//! 1. les octets sont bons et c'est l'**état** du meter en direct qui les a
//!    perdus — réassembleur gardant des octets périmés après la coupure de
//!    93,5 s, sous le seuil de péremption de 120 s qui n'a donc pas agi ;
//! 2. les octets eux-mêmes ne portent pas une fiche lisible.
//!
//! Ce rejeu part d'un réassembleur **neuf**. S'il lit la fiche, l'explication 1
//! tient et le défaut est dans la gestion d'état du direct. S'il ne la lit pas,
//! c'est l'explication 2 et il faut regarder le décodage.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test rejeu_fiche -- --ignored --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::stream_assembler::StreamAssembler;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};
use xiiinrv_meter_lib::xiiinrv::collecte;

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[test]
#[ignore = "diagnostic"]
fn que_lit_la_fiche_dans_cette_capture() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    collecte::vider();
    collecte::nom_detecte(None);
    collecte::ouvrir_lecture(true);

    let storage = Arc::new(DataStorage::new());
    let mut flux: HashMap<String, (StreamAssembler, StreamProcessor)> = HashMap::new();
    let (mut lignes, mut octets_total) = (0usize, 0usize);

    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
        if bouts.len() != 3 {
            continue;
        }
        let Some(donnees) = decode_hex(bouts[2]) else { continue };
        lignes += 1;
        octets_total += donnees.len();

        let (assembleur, processeur) = flux.entry(bouts[1].to_string()).or_insert_with(|| {
            (
                StreamAssembler::new(),
                StreamProcessor::new(
                    storage.clone(),
                    Arc::new(SkillLookup::new()),
                    Arc::new(NpcLookup::new()),
                ),
            )
        });
        assembleur.process_chunk(&donnees, processeur);
    }

    let e = collecte::lire_etat();
    println!("\n{lignes} lignes, {octets_total} octets rejoués depuis {chemin}");
    println!("\nce que la fiche en retire :");
    println!("  nom           : {:?}", e.nom);
    println!("  serveur       : {:?}", e.serveur);
    println!("  niveau        : {:?}", e.niveau);
    println!("  Item Level    : {:?}", e.item_level);
    println!("  Combat Power  : {:?}", e.combat_power);
    println!("  PV / PM       : {:?} / {:?}", e.pv, e.pm);
    println!("  équipement    : {} pièces", e.equipement.len());
    println!("  pets          : {} familles", e.pets.len());

    if !e.equipement.is_empty() {
        println!("
  pièces trouvées (emplacement, identifiant, enchantement) :");
        for p in &e.equipement {
            println!("    emplacement {:>3}  id {:>10}  +{}", p.emplacement, p.item_id, p.enchantement);
        }
    }

    collecte::ouvrir_lecture(false);
    collecte::vider();
    collecte::nom_detecte(None);
}
