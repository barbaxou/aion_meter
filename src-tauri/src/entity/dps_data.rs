use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::personal_data::PersonalData;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DpsData {
    pub map: HashMap<i32, PersonalData>,
    pub target_name: String,
    pub target_mode: String,
    pub target_id: i32,
    pub battle_time: i64,
    pub local_player_id: Option<i64>,
    /// Max HP of the current single boss target (0 = unknown / multi-target).
    pub target_max_hp: i64,
    /// Cumulative tracked damage dealt to the current target — the fallback HP-bar
    /// source (max_hp - dealt) when no live current-HP reading is available.
    pub target_total_damage: i64,
    /// Live current HP of the current target from the in-place HP feed, or -1 if
    /// none has been observed. When >= 0 the bar uses this real value directly.
    pub target_current_hp: i64,
    /// Instance id from the party roster (0 = not in a party instance). The
    /// frontend maps it to a dungeon name + difficulty.
    pub dungeon_id: i32,

    /// Ajout XIII NRV : la part des dégâts réellement subis par la cible que
    /// nous avons comptée, en pourcentage. `None` tant que le jeu ne nous a pas
    /// donné les PV courants de la cible.
    ///
    /// **C'est notre moyen de nous juger nous-mêmes.** Le jeu envoie les PV
    /// courants de la cible : ce qu'elle a réellement perdu vaut donc
    /// `target_max_hp - target_current_hp`, tous assaillants confondus. Comparé
    /// à ce que nous avons suivi, l'écart dit en direct ce qui nous échappe.
    ///
    /// Le projet traîne un déficit de dégâts en groupe (−6 à −35 % selon les
    /// sessions) qu'il fallait jusqu'ici mesurer à la main, en relevant
    /// l'analyseur du jeu après coup. Ce chiffre le rend visible pendant le
    /// combat, sans rien comparer.
    ///
    /// Au-dessus de 100 % n'est pas une anomalie : les coups qui tombent à
    /// l'instant de la mort, ou le surplus du dernier coup, dépassent le
    /// réservoir de vie.
    pub hp_coverage: Option<f64>,
    /// Ajout XIII NRV : les PV courants comblés entre deux lectures du jeu, pour
    /// que la barre descende sans à-coup. `-1` quand aucune lecture n'est
    /// arrivée.
    ///
    /// Séparé de `target_current_hp`, qui reste la lecture brute : c'est elle
    /// que `hp_coverage` compare à nos chiffres, et la mélanger à nos propres
    /// coups ferait dire 100 % au contrôle quoi qu'il arrive.
    pub smoothed_current_hp: i64,
}

impl DpsData {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            target_name: String::new(),
            target_mode: "bossTargets".to_string(),
            target_id: 0,
            battle_time: 0,
            local_player_id: None,
            target_max_hp: 0,
            target_total_damage: 0,
            target_current_hp: -1,
            dungeon_id: 0,
            hp_coverage: None,
            smoothed_current_hp: -1,
        }
    }
}
