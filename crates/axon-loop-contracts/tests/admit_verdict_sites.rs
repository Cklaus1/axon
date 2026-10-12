//! C9 round 4c, ADMIT (amendment 76): library tests for the one arm of
//! `project_receipt_status` whose every production route is dominated by
//! another check (a LIBRARY_PRIMITIVE row, ruling R1, never counted killed).

use axon_loop_contracts::*;

/// `project_receipt_status` maps an execution receipt's status to the episode
/// status `bind_acf` compares. A canceled execution is a Cancelled episode,
/// never a Completed one. Its one production caller, evl `judge`, then refuses a
/// canceled run's verdict on its own (`run_end`, M131).
#[test]
fn a_canceled_execution_receipt_never_projects_to_a_completed_episode() {
    if project_receipt_status(ReceiptStatus::Canceled) == EpisodeStatus::Completed {
        panic!("ATTACK: a canceled execution receipt projected to a completed episode status");
    }
    assert_eq!(
        project_receipt_status(ReceiptStatus::Canceled),
        EpisodeStatus::Cancelled
    );
}
