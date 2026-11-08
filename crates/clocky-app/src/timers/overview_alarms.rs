//! M7a: the alarm cards of Ativos, merged with the timer cards by urgency (both rank on the
//! scale of [`super::cards::rank`]; on a tie the timer comes first).

use std::time::Duration;

use super::convert::row_to_ui;
use super::frame::{Source, task_line};
use crate::alarms::AlarmCard;
use crate::ui;

/// A row of the Ativos list with its urgency.
pub type Ranked = ((u8, Duration), ui::ActiveCard);

/// The Slint row of an alarm card.
pub fn alarm_row(card: &AlarmCard, src: &Source<'_>) -> Ranked {
    let row = ui::ActiveCard {
        id: row_to_ui(card.id.0),
        kind: "alarm".into(),
        name: card.name.as_str().into(),
        status: card.status.as_str().into(),
        time: card.time.as_str().into(),
        ringing: card.ringing,
        enabled: card.enabled,
        task: task_line(src.names.task(card.task)),
        ..ui::ActiveCard::default()
    };
    (card.rank, row)
}

/// Timer rows (already in order) and alarm rows, merged by rank.
pub fn merge(timers: Vec<Ranked>, mut alarms: Vec<Ranked>) -> Vec<ui::ActiveCard> {
    alarms.sort_by_key(|(rank, _)| *rank);
    let mut rows = Vec::with_capacity(timers.len() + alarms.len());
    let mut alarms = alarms.into_iter().peekable();
    for (rank, row) in timers {
        while let Some((_, alarm)) = alarms.next_if(|(alarm_rank, _)| *alarm_rank < rank) {
            rows.push(alarm);
        }
        rows.push(row);
    }
    rows.extend(alarms.map(|(_, row)| row));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, rank: (u8, u64)) -> Ranked {
        let card = ui::ActiveCard {
            name: name.into(),
            ..ui::ActiveCard::default()
        };
        ((rank.0, Duration::from_secs(rank.1)), card)
    }

    #[test]
    fn alarms_slot_in_by_urgency_and_ties_keep_timers_first() {
        let timers = vec![
            row("ringing tea", (0, 0)),
            row("pipeline", (2, 600)),
            row("sw", (4, 0)),
        ];
        let alarms = vec![
            row("off", (7, 0)),
            row("standup", (2, 300)),
            row("ringing alarm", (0, 0)),
            row("tomorrow", (6, 9000)),
        ];
        let names: Vec<String> = merge(timers, alarms)
            .into_iter()
            .map(|c| c.name.to_string())
            .collect();
        assert_eq!(
            names,
            [
                "ringing tea",
                "ringing alarm",
                "standup",
                "pipeline",
                "sw",
                "tomorrow",
                "off"
            ]
        );
    }
}
