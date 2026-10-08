//! Deux tentatives sur le même boss comptent-elles séparément ?
//!
//! Le 08/10/2026, « Désir de Kromede » a été enregistré avec 21 159 960 dégâts
//! pour un boss de 15 504 000 PV — **136 %**, sur 454 secondes. Le groupe était
//! tombé à 65 %, était mort, et avait recommencé : les deux tentatives
//! s'additionnaient.
//!
//! Ce rejeu lit la capture réelle dans le vrai chemin et dit ce que le meter
//! compte sur cette entité à la fin. Avec la correction, le total doit valoir
//! la seconde tentative seule — c'est-à-dire environ les PV du boss.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test deux_tentatives -- --ignored --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::stream_assembler::StreamAssembler;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};

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
fn que_compte_t_on_sur_un_boss_recommence() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    let storage = Arc::new(DataStorage::new());
    let mut flux: HashMap<String, (StreamAssembler, StreamProcessor)> = HashMap::new();
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

    let combat = storage.get_combat_snapshot_light();
    let mut cibles: Vec<(i32, i64, i32)> = combat
        .iter()
        .map(|(&id, td)| (id, td.total_damage, storage.get_mob_hp(id).unwrap_or(0)))
        .filter(|(_, d, pv)| *pv > 1_000_000 || *d > 1_000_000)
        .collect();
    cibles.sort_by_key(|(_, d, _)| std::cmp::Reverse(*d));

    println!("\n{chemin}");
    println!("\n  {:>9}  {:>12}  {:>12}  {:>7}", "entité", "dégâts", "PV max", "taux");
    for (id, degats, pv) in cibles.iter().take(12) {
        println!(
            "  {id:>9}  {degats:>12}  {pv:>12}  {:>6.0} %",
            100.0 * *degats as f64 / *pv as f64
        );
    }
    println!(
        "\n  Un taux proche de 100 % est sain. Nettement au-dessus, deux\n  \
         tentatives se sont additionnées."
    );
}
