//! Work is identified by window + stable tab id + pane, never a list index.
//! Status comes from shell marks and explicit attention, never output silence.
use crate::app::{App, Pane};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status { NeedsInput, Attention, Failed, Running, Finished, Idle }

impl Status {
    pub fn label(self) -> &'static str {
        match self { Self::NeedsInput => "NEEDS INPUT", Self::Attention => "NEEDS ATTENTION", Self::Failed => "FAILED", Self::Running => "RUNNING", Self::Finished => "FINISHED", Self::Idle => "SHELL" }
    }
    pub fn attention(self) -> bool { matches!(self, Self::NeedsInput | Self::Attention | Self::Failed) }
    fn rank(self) -> u8 { match self { Self::NeedsInput => 0, Self::Attention => 1, Self::Failed => 2, Self::Running => 3, Self::Finished => 4, Self::Idle => 5 } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Target { pub window: u64, pub tab: u64, pub right: bool }

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Item {
    pub target: Target,
    pub title: String,
    pub command: String,
    pub space: String,
    pub cwd: String,
    pub status: Status,
    pub exit: Option<i32>,
    pub progress: Option<u8>,
    pub unread: bool,
}

pub fn classify(running: bool, attention: bool, input: bool, completed: bool, exit: Option<i32>) -> Status {
    if input { Status::NeedsInput }
    else if completed && !running && exit.is_some_and(|n| n != 0) { Status::Failed }
    else if attention && (running || !completed) { Status::Attention }
    else if running { Status::Running }
    else if completed { Status::Finished }
    else { Status::Idle }
}

pub fn collect(app: &App) -> Vec<Item> {
    let mut items = Vec::new();
    for tab in app.tabs.iter().filter(|t| t.peek.is_none()) {
        for (right, pane) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
            let Pane::Term(t) = pane else { continue };
            let running = t.running_since.is_some();
            let completed = t.work_completed;
            let exit = if completed { t.last_exit } else { None };
            let command = t.work_command.clone();
            items.push(Item {
                target: Target { window: u64::from(app.window.id()), tab: tab.id, right },
                title: tab.name.clone().unwrap_or_else(|| if command.is_empty() { t.title.clone() } else { command.clone() }),
                command, space: app.space_name.clone(), cwd: t.term.cwd.clone().or_else(|| t.cwd.clone()).unwrap_or_else(|| t.profile_name.clone()),
                status: classify(running, t.waiting, t.confirm_paste.is_some() || t.confirm_close.is_some(), completed, exit),
                exit, progress: t.progress.filter(|(s, _)| *s == 1).map(|(_, p)| p.min(100)), unread: t.waiting,
            });
        }
    }
    items
}

pub fn sort(items: &mut [Item]) { items.sort_by_key(|i| (i.status.rank(), i.target.window, i.target.tab, i.target.right)); }

/// Only a newly observed completion merits a notice. Restored history does not.
pub fn completion<'a>(before: &[Item], after: &'a [Item]) -> Option<&'a Item> {
    after.iter().find(|item| item.unread && matches!(item.status, Status::Finished | Status::Failed)
        && before.iter().any(|old| old.target == item.target && matches!(old.status, Status::Running | Status::Attention | Status::NeedsInput)))
}

pub fn summary(items: &[Item]) -> String {
    let running = items.iter().filter(|i| i.status == Status::Running).count();
    let attention = items.iter().filter(|i| i.status.attention() && (i.unread || matches!(i.status, Status::NeedsInput | Status::Attention))).count();
    let finished = items.iter().filter(|i| i.status == Status::Finished && i.unread).count();
    let mut bits = Vec::new();
    if running > 0 { bits.push(format!("{running} running")); }
    if attention > 0 { bits.push(format!("{attention} need attention")); }
    if finished > 0 { bits.push(format!("{finished} finished")); }
    if bits.is_empty() { "Hatch · ready".into() } else { bits.join(" · ") }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn completion_is_not_a_request_for_input() {
        assert_eq!(classify(false, true, false, true, Some(0)), Status::Finished);
        assert_eq!(classify(false, true, false, true, Some(7)), Status::Failed);
        assert_eq!(classify(false, false, false, true, None), Status::Finished);
    }
    #[test] fn attention_requires_a_signal() {
        assert_eq!(classify(true, false, false, false, None), Status::Running);
        assert_eq!(classify(true, true, false, false, None), Status::Attention);
        assert_eq!(classify(true, false, true, false, None), Status::NeedsInput);
        assert_eq!(classify(false, false, false, false, None), Status::Idle);
    }
    #[test] fn a_new_command_replaces_the_old_failure() {
        assert_eq!(classify(true, false, false, true, Some(1)), Status::Running);
    }
    #[test] fn notices_require_an_unseen_completion_of_existing_work() {
        let mut item=Item{target:Target{window:1,tab:2,right:false},title:"Build".into(),command:"make".into(),space:String::new(),cwd:String::new(),status:Status::Running,exit:None,progress:None,unread:false};
        let before=vec![item.clone()];item.status=Status::Failed;item.exit=Some(1);item.unread=true;
        assert!(completion(&before,&[item.clone()]).is_some());
        assert!(completion(&[],&[item.clone()]).is_none(),"launch must not replay notices");
        assert!(completion(&[item.clone()],&[item.clone()]).is_none(),"same completion must not repeat");
        item.unread=false;assert!(completion(&before,&[item]).is_none(),"viewed work needs no notice");
    }
    #[test] fn stable_targets_do_not_depend_on_sort_order() {
        let mut rows: Vec<_> = [Status::Running, Status::Failed, Status::NeedsInput].into_iter().enumerate().map(|(n,status)| Item {target:Target{window:3,tab:n as u64,right:n==2},title:String::new(),command:String::new(),space:String::new(),cwd:String::new(),status,exit:None,progress:None,unread:true}).collect();
        sort(&mut rows);
        assert_eq!(rows[0].target,Target{window:3,tab:2,right:true});
        assert_eq!(summary(&rows),"1 running · 2 need attention");
    }
}
