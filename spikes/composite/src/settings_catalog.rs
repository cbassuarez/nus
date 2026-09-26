//! Settings presentation lives beside the binding table. Each card is a
//! concrete value; independent switches get their own On / Off pair.
use super::*;

fn switch(hit: Hit) -> Option<(&'static str, Hit, Hit, bool)> {
    macro_rules! switches { ($($variant:ident => $title:literal),* $(,)?) => {
        match hit { $(Hit::$variant(v) => Some(($title, Hit::$variant(true), Hit::$variant(false), v)),)* _ => None }
    }; }
    switches! {
        HdrMasthead => "LARGE SIDEBAR TITLE", HdrDateline => "LOCATION & TAB COUNTS",
        HdrButton => "NEW TAB IN HEADER", HdrNextRow => "NEW TAB BELOW TABS",
        HdrName => "WINDOW NAMES", HdrCaret => "TAB TYPE MENU", HdrRailHover => "HIDE WINDOW RAIL UNTIL HOVER",
        HdrFlash => "PRESS FEEDBACK", Compact => "COMPACT SIDEBAR", Pin => "KEEP SIDEBAR VISIBLE",
        CloseAsks => "CONFIRM CLOSING BUSY TABS", ShellInt => "SHELL INTEGRATION", Block => "CONTENT BLOCKING",
        Highlight => "COMMAND COLORS", Predict => "COMMAND SUGGESTIONS", FormatOnSave => "FORMAT FILES ON SAVE",
        CopyOnSelect => "COPY SELECTED TEXT", MiddlePaste => "PASTE WITH MIDDLE CLICK", PageSmooth => "SMOOTH PAGE SCROLLING",
        PaneDivider => "RESIZE SPLIT PANES", Blocks => "COMMAND STATUS MARKERS", Journal => "REMEMBER COMMANDS I RUN",
        PortsRemember => "REMEMBER PORT LABELS", ClickToSource => "OPEN SOURCE FROM A PAGE", Remember => "REMEMBER OPEN TABS",
        HandsSubmit => "CONFIRM FORM SUBMISSION", ProgressSidebar => "SIDEBAR PROGRESS", ProgressTaskbar => "DOCK / TASKBAR PROGRESS",
        SshIntegration => "SHELL INTEGRATION OVER SSH", Selvedge => "LETTER PLACE EDGES", Dedupe => "DUPLICATE PAGE NOTICE", SyncSession => "SYNC OPEN TABS",
        SyncAtQuit => "SYNC WHEN QUITTING", PortsToast => "NEW PORT NOTIFICATIONS", PortsProbe => "DETECT WEB SERVERS",
        HatchAutohide => "HIDE HATCH WHEN UNFOCUSED", HatchStatus => "COMPACT WORK STATUS", HatchBackground => "KEEP NUS IN BACKGROUND", HatchDim => "DIM BEHIND MODAL", HatchNotify => "COMPLETION NOTICES", Phone => "PHONE ACCESS", SoundOn => "APP SOUNDS",
        StartupSound => "LAUNCH SOUND", MenuEnabled=>"MENU BAR / TRAY ICON", MenuNames=>"SHOW TASK & FILE NAMES", MenuRecent=>"INCLUDE FINISHED ITEMS"
    }
}

pub(super) fn is_action(hit: Hit) -> bool {
    matches!(hit, Hit::Workspace(_) | Hit::Play(_) | Hit::MeEdit(_) | Hit::MeCard | Hit::MeFolder | Hit::MeForget | Hit::MeWalk(_) |
        Hit::SyncKey | Hit::SyncEdit(_) | Hit::SyncForget | Hit::SyncNow | Hit::ForgeForget |
        Hit::CopyPhoneUrl | Hit::HandsForget | Hit::ForgetMemory | Hit::PortsHidden | Hit::Starter(_) |
        Hit::Search | Hit::FooterDefaults | Hit::PlaceEdit | Hit::Welcome | Hit::MenuPreview | Hit::MenuMove(..) | Hit::ReloadRules | Hit::OpenRules | Hit::ResetRules | Hit::MakeDefault | Hit::Unregister | Hit::Widevine |
        Hit::ReloadAvatar | Hit::OpenProfileDir | Hit::Section(_))
}

fn action_icon(hit: Hit) -> (&'static str, &'static str) {
    match hit {
        Hit::Play(_) => icons::PLAY,
        Hit::MeEdit(_) => icons::PENCIL,
        Hit::MeCard | Hit::MeWalk(_) => icons::USER,
        Hit::MeFolder => icons::FOLDER,
        Hit::SyncNow => icons::BROADCAST,
        Hit::SyncKey | Hit::CopyPhoneUrl => icons::COPY,
        Hit::SyncEdit(_) => icons::PENCIL,
        Hit::HandsForget | Hit::ForgetMemory | Hit::SyncForget | Hit::MeForget | Hit::ForgeForget => icons::WARNING,
        Hit::PortsHidden => icons::RELOAD,
        _ => icons::OPEN_EXTERNAL,
    }
}

fn description(hit: Hit) -> Option<String> {
    let text: String = match hit {
        Hit::MenuEnabled(true)=>"Show nus in your menu bar or tray.",Hit::MenuEnabled(false)=>"Use the drawer from the nus footer only.",
        Hit::MenuSignal(crate::menu_drawer::SignalStyle::Dot)=>"A quiet mark with an activity or attention dot.",
        Hit::MenuSignal(crate::menu_drawer::SignalStyle::Count)=>"Show the number of active tasks and downloads.",
        Hit::MenuSignal(crate::menu_drawer::SignalStyle::Text)=>"Show a short status, such as 2 active.",
        Hit::MenuDensity(_,crate::menu_drawer::Density::Hidden)=>"Leave this section out of the drawer.",
        Hit::MenuDensity(_,crate::menu_drawer::Density::Compact)=>"Use short rows, like the Signal work menu.",
        Hit::MenuDensity(_,crate::menu_drawer::Density::Expanded)=>"Use Desk cards with details and controls.",
        Hit::MenuNames(true)=>"Show project and download names in the drawer.",Hit::MenuNames(false)=>"Use generic labels; keep status and controls.",
        Hit::MenuRecent(true)=>"Include recent completed tasks and downloads.",Hit::MenuRecent(false)=>"Show active work and downloads only.",
        Hit::HdrStyle(HeaderStyle::Bar) => "Window name above the tab list.",
        Hit::HdrStyle(HeaderStyle::Rail) => "Window squares along the edge.",
        Hit::HdrMasthead(true) => "Use a large serif window title.", Hit::HdrMasthead(false) => "Use a compact uppercase title.",
        Hit::HdrDateline(true) => "Show location, tab and port counts.", Hit::HdrDateline(false) => "Leave out the extra status line.",
        Hit::HdrButton(true) => "Add a + button above your tabs.", Hit::HdrButton(false) => "Remove the header + button.",
        Hit::HdrNextRow(true) => "Add New tab below the last tab.", Hit::HdrNextRow(false) => "End the list at the last tab.",
        Hit::HdrName(true) => "Show names beside window icons.", Hit::HdrName(false) => "Show only each window's icon.",
        Hit::HdrCaret(true) => "Show a menu for new tab types.", Hit::HdrCaret(false) => "Hide the tab type arrow.",
        Hit::HdrRailHover(true) => "Reveal windows near the edge.", Hit::HdrRailHover(false) => "Keep the window rail visible.",
        Hit::HdrFlash(true) => "Briefly highlight pressed controls.", Hit::HdrFlash(false) => "No flash when controls are pressed.",
        Hit::Compact(true) => "Narrow sidebar with icons only.", Hit::Compact(false) => "Show tab icons and their titles.",
        Hit::PinDisplay(crate::pins::Display::Icon) => "Use a favicon, or an icon for shells and built-in pages.",
        Hit::PinDisplay(crate::pins::Display::Preview) => "Show the open web page inside its pinned tile.",
        Hit::SmallTabs(crate::sidebar::SmallTabs::Icons) => "Use an icon for each tab type.",
        Hit::SmallTabs(crate::sidebar::SmallTabs::Favicons) => "Use each website's own icon.",
        Hit::SmallTabs(crate::sidebar::SmallTabs::Preview) => "Show a miniature page preview.",
        Hit::DownloadRename(crate::downloads::Rename::Off) => "Keep the site's filename.",
        Hit::DownloadRename(crate::downloads::Rename::All) => "Use readable page titles.",
        Hit::DownloadRename(crate::downloads::Rename::Selective) => "Keep technical names intact.",
        Hit::Pin(true) => "Keep the sidebar open.", Hit::Pin(false) => "Reveal the sidebar on hover.",
        Hit::Side(Side::Left) => "Place tabs on the left edge.", Hit::Side(Side::Right) => "Place tabs on the right edge.",
        Hit::HoverFrom(HoverFrom::ScreenEdge) => "Reveal at the edge of the screen.", Hit::HoverFrom(HoverFrom::InsideWindow) => "Reveal only inside this window.",
        Hit::Fullscreen(Fullscreen::Hover) => "Reveal tabs near the edge.", Hit::Fullscreen(Fullscreen::Hidden) => "Keep tabs hidden in fullscreen.", Hit::Fullscreen(Fullscreen::Pinned) => "Keep tabs visible in fullscreen.",
        Hit::SwipeLook(crate::settings::SwipeLook::Arrow) => "A disc with an arrow at the page edge.", Hit::SwipeLook(crate::settings::SwipeLook::Card) => "The arrow, and what the swipe will do.", Hit::SwipeLook(crate::settings::SwipeLook::Edge) => "A band that grows down the page edge.", Hit::SwipeLook(crate::settings::SwipeLook::Off) => "Swipe without an overlay.",
        Hit::SwipeReach(120) => "Fire after a short swipe.", Hit::SwipeReach(180) => "Fire after a medium swipe.", Hit::SwipeReach(_) => "Fire only after a long swipe.",
        Hit::OpenedBy(OpenedBy::Behind) => "Keep working; announce new tabs.", Hit::OpenedBy(OpenedBy::Front) => "Switch to externally opened tabs.",
        Hit::Links(Links::Stack) => "Nest linked pages under this tab.", Hit::Links(Links::Split) => "Show linked pages beside this one.", Hit::Links(Links::NewTab) => "Open linked pages as separate tabs.",
        Hit::PromptUrl(PromptUrl::Split) => "Open typed URLs beside the shell.", Hit::PromptUrl(PromptUrl::NewTab) => "Open typed URLs in their own tabs.",
        Hit::CloseAsks(true) => "Ask before stopping a busy shell.", Hit::CloseAsks(false) => "Close busy shells immediately.",
        Hit::PaneControls(crate::panes::Controls::Near) => "Reveal controls near a pane corner.", Hit::PaneControls(crate::panes::Controls::Never) => "Hide the corner controls.",
        Hit::PaneDivider(true) => "Drag the divider to resize panes.", Hit::PaneDivider(false) => "Keep the divider in place.",
        Hit::DefaultProfile(_) => "Use this shell for new terminals.",
        Hit::PromptLsp(PromptLsp::Quiet) => "Underline errors; suggest inline.", Hit::PromptLsp(PromptLsp::Menu) => "Show a completion menu as you type.", Hit::PromptLsp(PromptLsp::Off) => "No language-server suggestions.",
        Hit::LinkClick(LinkClick::Ask) => "Ask before opening terminal links.", Hit::LinkClick(LinkClick::Open) => "Open terminal links immediately.", Hit::LinkClick(LinkClick::HintsOnly) => "Open only through keyboard hints.",
        Hit::CutOffMode(CutOff::Chip) => "Offer to resume interrupted work.", Hit::CutOffMode(CutOff::RunAgain) => "Automatically rerun interrupted commands.", Hit::CutOffMode(CutOff::Off) => "Do not offer or rerun old commands.",
        Hit::KeepAlive(KeepAlive::On) => "New shells can survive quitting nus.", Hit::KeepAlive(KeepAlive::Off) => "New shells stop when nus quits.",
        Hit::ShellColours(ShellColours::Chip) => "Offer program colors as a theme.", Hit::ShellColours(ShellColours::Always) => "Apply program colors to the app.", Hit::ShellColours(ShellColours::PaneOnly) => "Apply colors only in that terminal.",
        Hit::Grade(Grade::Off) => "Keep the program's original contrast.", Hit::Grade(g) => return Some(format!("Adjust text to at least {}:1 contrast.",g.ratio())),
        Hit::Truecolour(Truecolour::AsSent) => "Keep each program's own palette.", Hit::Truecolour(Truecolour::Snapped) => "Match colors to your theme palette.",
        Hit::Osc52(Osc52::Off) => "Deny program clipboard access.", Hit::Osc52(Osc52::Write) => "Programs may copy, but cannot read.", Hit::Osc52(Osc52::ReadWrite) => "Programs may copy and read clipboard.",
        Hit::WheelLines(n) => return Some(format!("Move {n} lines per wheel tick.")),
        Hit::ScrollEasing(crate::scrolling::Easing::Instant) => "Move straight to the new position.",
        Hit::ScrollEasing(_) => "Follow the curve shown above.",
        Hit::PortsGrouping(PortsGrouping::Origin) => "Group by who started each service.", Hit::PortsGrouping(PortsGrouping::Port) => "Sort by port number.", Hit::PortsGrouping(PortsGrouping::Process) => "Group by the running program.",
        Hit::PortsOpen(PortsOpen::Tab) => "Open the service in a new tab.", Hit::PortsOpen(PortsOpen::Split) => "Open the service beside this pane.", Hit::PortsOpen(PortsOpen::Peek) => "Preview the service in an overlay.",
        Hit::PortsKill(KillConfirm::System) => "Ask for processes nus did not start.", Hit::PortsKill(KillConfirm::Always) => "Ask before stopping any process.", Hit::PortsKill(KillConfirm::Never) => "Stop processes without asking.",
        Hit::PortsTunnel(Tunnel::Cloudflared) => "Share with installed cloudflared.", Hit::PortsTunnel(Tunnel::Ngrok) => "Share with installed ngrok.",
        Hit::HatchSpaces(HatchSpaces::Follow) => "Use the hatch for the current space.", Hit::HatchSpaces(HatchSpaces::One) => "Share one hatch across spaces.",
        Hit::HatchHotkey(_) => "Press this shortcut to show the hatch.",
        Hit::BarStyle(BarStyle::Radiance) => "A continuous bar with an HDR highlight on supported displays; holds when loading stalls.",
        Hit::BarStyle(BarStyle::Rule) => "A solid line grows with loading.", Hit::BarStyle(BarStyle::Comet) => "A bright head with a fading trail.", Hit::BarStyle(BarStyle::Carapace) => "Loading fills the window frame.",
        Hit::BarColor(BarColor::Signal) => "Use the theme accent color.", Hit::BarColor(BarColor::Tab) => "Use this tab's assigned color.", Hit::BarColor(BarColor::Ink) => "Use the theme text color.",
        Hit::StatusStyle(Status::Lamp) => "Show a small page-status indicator.", Hit::StatusStyle(Status::Both) => "Show an indicator and status text.", Hit::StatusStyle(Status::Word) => "Show status as text only.", Hit::StatusStyle(Status::None) => "Hide the page-status indicator.",
        Hit::SleepAfter(n) => return Some(if n == 0 { "Keep idle pages running.".into() } else { format!("Pause pages after {n} idle minutes.") }),
        Hit::ArchiveAfter(n) => return Some(if n == 0 { "Keep idle pages in the sidebar.".into() } else { format!("Close pages after {n} idle hours.") }),
        Hit::TidyEvery(TidyEvery::Off) => "Suggest groups only when asked.", Hit::TidyEvery(TidyEvery::Hourly) => "Suggest tab groups every hour.", Hit::TidyEvery(TidyEvery::Daily) => "Suggest tab groups once a day.",
        Hit::Dedupe(true) => "Offer to switch to an existing copy.", Hit::Dedupe(false) => "Open duplicates without a notice.",
        Hit::ShellInt(true) => "Track commands in new shells.", Hit::ShellInt(false) => "Start new shells without hooks.",
        Hit::Highlight(true) => "Color commands as you type.", Hit::Highlight(false) => "Use plain command text.",
        Hit::Predict(true) => "Suggest commands from history.", Hit::Predict(false) => "Do not suggest previous commands.",
        Hit::FormatOnSave(true) => "Run a formatter when saving files.", Hit::FormatOnSave(false) => "Save the file exactly as edited.",
        Hit::Blocks(true) => "Mark each command's status.", Hit::Blocks(false) => "Hide command status markers.",
        Hit::FoldOver(n) => return Some(if n == 0 { "Keep all command output expanded.".into() } else { format!("Fold output longer than {n} lines.") }),
        Hit::Journal(true) => "Remembers each command you run, so you can find and rerun it later.", Hit::Journal(false) => "New commands aren't remembered. What's already saved expires on schedule.",
        Hit::JournalKeep(n) => return Some(format!("Forget commands after {n} days.")),
        Hit::Replay(ReplayKeep::Days7) => "Record replay; keep seven days.", Hit::Replay(ReplayKeep::Day1) => "Record replay; keep one day.", Hit::Replay(ReplayKeep::Off) => "Stop recording new replay data.",
        Hit::CopyOnSelect(true) => "Copy terminal selections immediately.", Hit::CopyOnSelect(false) => "Copy only when you ask.",
        Hit::MiddlePaste(true) => "Middle-click pastes the clipboard.", Hit::MiddlePaste(false) => "Middle-click does not paste.",
        Hit::PageSmooth(true) => "Animate scrolling after restart.", Hit::PageSmooth(false) => "Jump directly after restart.",
        Hit::Block(true) => "Block known ads and trackers.", Hit::Block(false) => "Allow requests on the block list.",
        Hit::ClickToSource(true) => "Offer source links on local pages.", Hit::ClickToSource(false) => "Keep normal page clicks.",
        Hit::Ledger(true) => "Show what assistants are doing under their tabs.", Hit::Ledger(false) => "Show assistants as a dot beside the tab.",
        Hit::ProgressSidebar(true) => "Show task progress beside tabs.", Hit::ProgressSidebar(false) => "Hide progress in the sidebar.",
        Hit::ProgressTaskbar(true) => "Show progress on the app icon.", Hit::ProgressTaskbar(false) => "Keep the app icon unchanged.",
        Hit::SshIntegration(true) => "Install hooks for remote commands.", Hit::SshIntegration(false) => "Leave remote shells unchanged.",
        Hit::Selvedge(true) => "Letter the edge of shells that run elsewhere.", Hit::Selvedge(false) => "Draw remote shells like local ones.",
        Hit::Hands(HandsMode::Ask) => "Ask before assistant actions.", Hit::Hands(HandsMode::Always) => "Allow actions without asking.", Hit::Hands(HandsMode::Never) => "Prevent assistant actions.",
        Hit::HandsSubmit(true) => "Always ask before submitting forms.", Hit::HandsSubmit(false) => "Use the site's existing permission.",
        Hit::AskCtx(c) => return Some(c.words().replace('·', "—")),
        Hit::SyncSession(true) => "Include open tabs in profile sync.", Hit::SyncSession(false) => "Keep open tabs on this device.",
        Hit::SyncEvery(n) => return Some(if n == 0 { "Sync only when you choose.".into() } else { format!("Sync every {n} minutes when set up.") }),
        Hit::SyncAtQuit(true) => "Attempt a final sync when quitting.", Hit::SyncAtQuit(false) => "Quit without starting a sync.",
        Hit::Phone(true) => "Serve a phone view on your network.", Hit::Phone(false) => "Stop access; revoke the old link.",
        Hit::PortsToast(true) => "Notify when a new port appears.", Hit::PortsToast(false) => "Update the list without a toast.",
        Hit::PortsProbe(true) => "Send HTTP requests to identify apps.", Hit::PortsProbe(false) => "List ports without probing them.",
        Hit::PortsShow(k, on) => return Some(format!("{} {} in the port list.", if on { "Include" } else { "Hide" }, ["system services", "UDP listeners", "connections", "Docker ports"][k.min(3) as usize])),
        Hit::PortsPoll(n) => return Some(format!("Refresh the port list every {n}s.")),
        Hit::HatchLook(HatchLook::Sheet) => "A terminal sheet from the top.", Hit::HatchLook(HatchLook::Card) => "A floating terminal card.",
        Hit::HatchSize(n) => return Some(format!("Use {n}% of the screen height.")),
        Hit::HatchAutohide(true) => "Dismiss when another app is focused.", Hit::HatchAutohide(false) => "Keep the hatch visible until hidden.",
        Hit::HatchMonitor(HatchMonitor::Pointer) => "Open on the pointer's screen.", Hit::HatchMonitor(HatchMonitor::Foreground) => "Open on this nus window's screen.", Hit::HatchMonitor(HatchMonitor::Primary) => "Always open on the main screen.",
        Hit::Play(_) => "Play a preview of this sound.",
        Hit::MeEdit(0) => "Edit the name shown in your profile.", Hit::MeEdit(1) => "Choose an initial, emoji or image.", Hit::MeEdit(_) => "Name this device for sync.",
        Hit::MeForget => "Remove the profile identity and key.", Hit::MeCard => "Set up your name and picture.",
        Hit::MeFolder | Hit::OpenProfileDir => "Open the local profile folder.",
        Hit::MeWalk(0) => "Learn how the local profile works.", Hit::MeWalk(_) => "Connect a private sync repository.",
        Hit::SyncKey => "Create or copy your encryption key.", Hit::SyncEdit(0) => "Choose a folder for encrypted sync.", Hit::SyncEdit(1) => "Set the repository used for sync.", Hit::SyncEdit(_) => "Enter a key from another device.",
        Hit::SyncForget => "Forget this device's sync key.", Hit::SyncNow => "Send and receive profile changes.",
        Hit::HandsForget => "Clear remembered site permissions.", Hit::ForgetMemory => "Delete saved assistant memory.", Hit::PortsHidden => "Reset the hidden process list.",
        Hit::Starter(_) => "Replace the tab and window rules.", Hit::ResetRules => "Replace rules with the defaults.", Hit::ReloadRules => "Read changes from the rules file.", Hit::OpenRules => "Edit the rules in a new tab.",
        Hit::MakeDefault => "Ask the OS to open links with nus.", Hit::Unregister => "Remove nus's browser registration.",
        Hit::Widevine => "Download the Widevine module for DRM video now.",
        Hit::ForgetPasswords => "Delete every sign-in saved in this profile.",
        Hit::Welcome => "Open onboarding and the app guide.", Hit::SoundOn(true) => "Play sounds for app events.", Hit::SoundOn(false) => "Mute app event sounds.",
        _ => return None,
    }.into();
    Some(text)
}

impl App {
    pub(super) fn visual_settings(&self, section: usize, rows: Vec<(String, Control)>) -> Vec<(String, Control)> {
        if section == 0 || section == 2 { return rows; }
        let rows = if section == 5 {
            let mut source = rows;
            let mut ordered = Vec::new();
            for (heading,names) in [
                ("SHELLS", vec!["DEFAULT SHELL","SHELLS","SHELL INTEGRATION","KEEP ALIVE","SSH","PLACES"]),
                ("COMMAND EDITING", vec!["COMMAND LINE","PROMPT LSP","LANGUAGE SERVERS","EDITOR","BLOCKS","CLICK LINKS"]),
                ("CLIPBOARD & SCROLLING",vec!["CLIPBOARD","OSC 52","SCROLL","WHEEL","SCROLLBACK"]),
                ("HISTORY & REPLAY",vec!["JOURNAL","CUT OFF","REPLAY"]),
                ("COLORS & PROGRESS",vec!["SHELL COLORS","PROGRAM COLORS","TRUECOLOR","PROGRESS"]),
            ] {
                ordered.push((heading.into(),Control::Caption));
                let intro=match heading {
                    "COLORS & PROGRESS"=>Some("Programs don't know your theme, and a long command can't tell you how far along it is. nus reads what they send the terminal and makes it fit: colors a script sets, colors a program hardcodes, and the progress a command reports."),
                    _=>None,
                };
                if let Some(intro)=intro {ordered.push(("".into(),Control::Info(intro.into())));}
                for name in names {
                    if let Some(i)=source.iter().position(|(label,_)|label==name) {ordered.push(source.remove(i));}
                    let note=match name {
                        "SHELLS"=>Some("Everything nus found on this machine, grouped, with how much of nus's integration each gets (hover a badge). Hidden ones stay here and in the palette; OPEN starts one now."),
                        "SHELL INTEGRATION"=>Some("Applies to new shells. Tracks the current folder, commands and exit codes so command navigation and status markers can work."),
                        "KEEP ALIVE"=>Some("Applies to new shells. Requires the nus-hold helper; existing shells keep the behavior they started with."),
                        "SSH"=>Some("When enabled, new SSH sessions copy shell integration scripts to ~/.cache/nus on the remote machine."),
                        "PLACES"=>Some("A shell that runs somewhere else (ssh, mosh, et, WSL) is lettered on its edge, so it reads as elsewhere without relying on colour. Names matching guarded_places (in settings.json; *prod* to start) get a red GUARDED banner."),
                        "PROMPT LSP"=>Some("A language server reads the command line as you type: Quiet underlines a problem and ghosts a completion (Tab accepts); Menu lists completions under the caret. It needs the server for your shell, below."),
                        "LANGUAGE SERVERS"=>Some("bash-language-server reads bash and zsh; PowerShell Editor Services reads PowerShell. GET installs one into this profile (a folder under profile/tools you can delete); shells pick it up without restarting."),
                        "EDITOR"=>Some("Formatting needs a formatter for the file type. When none is installed, the file is saved unchanged."),
                        "REPLAY"=>Some("Records terminal output and a snapshot of the page beside it at command checkpoints. Changes apply now; turning it off leaves existing recordings available."),
                        "JOURNAL"=>Some("Private to you: kept on this device, encrypted with a key in your system keychain, never synced or sent anywhere. For each finished command it saves the command line, the folder, when it ran, how long it took and whether it worked. It never saves output. It powers “Run again” suggestions, the log in the palette, and nus log. If you type secrets directly into commands (a password or token as an argument), they are saved too; turn this off or keep a shorter history."),
                        "SHELL COLORS"=>Some("When a script sets the terminal's colors (OSC 10/11: kitty's set-colors, a base16 script), nus can take them for the whole window: paper or ink from the background, the accent from the foreground. Offer puts a chip on the pane to apply them; Always applies them at once; Pane only keeps them in that terminal. nus theme <name> and nus look do the same on purpose."),
                        "PROGRAM COLORS"=>Some("claude, codex, htop and every TUI pick colors against someone else's background, and some of that text can't be read on yours. The grade moves only the unreadable text toward ink until it meets the contrast you pick (WCAG: 3:1, 4.5:1 AA, 7:1 AAA). Text that already reads is left alone."),
                        "TRUECOLOR"=>Some("Programs that send 24-bit color ignore your theme. The theme's sixteen snaps each of those colors to the nearest of the theme's sixteen, so every program wears the theme. In rules.luau, program(p) gives one program its own sixteen, remaps a color it hardcodes, or sets these per program."),
                        "PROGRESS"=>Some("A command can report how far along it is (OSC 9;4, as winget, some build tools and a one-line printf in a script do). nus draws it as a bar along the pane's top, and, with these on, as a line under the tab in the sidebar and on the taskbar button (Windows), so you can look away while it runs. Rules see it too: on_progress."),
                        "SCROLLBACK"=>Some("Lines a new shell keeps behind it; shells already open keep what they started with. Restored with the session."),
                        _=>None,
                    };
                    if let Some(note)=note {ordered.push(("".into(),Control::Info(note.into())));}
                }
            }
            ordered
        } else {rows};
        let live = crate::live::has_live(section);
        let mut out = Vec::new();
        for (title, control) in rows {
            let title = match (section, title.as_str()) {
                (3, "HEADER") => "WINDOW HEADER", (3, "REVEAL") => "WHERE HOVER REVEALS THE SIDEBAR",
                (3, "GRACE") => "DELAY BEFORE HIDING", (3, "FULLSCREEN") => "SIDEBAR IN FULLSCREEN",
                (4, "OPENED BY OTHERS") => "TABS OPENED BY OTHER APPS", (4, "TIDY") => "SUGGEST TAB GROUPS",
                (4, "DEDUPE") => "DUPLICATE PAGES", (5, "REPLAY") => "REPLAY RECORDING", (5,"OSC 52") => "PROGRAM CLIPBOARD ACCESS", (5,"PROMPT LSP") => "COMPLETIONS & DIAGNOSTICS", (5,"CUT OFF") => "INTERRUPTED COMMANDS", (5,"WHEEL") => "MOUSE WHEEL DISTANCE", (5,"SCROLL") => "SCROLL ANIMATION", (5,"SCROLLBACK") => "SCROLLBACK LINES", (5, "JOURNAL") => "COMMAND HISTORY",
                (7, "SHOW") => "VISIBLE PORT TYPES", (7, "KILL") => "CONFIRM STOPPING PROCESSES",
                (9, "HANDS") => "ASSISTANT ACTION PERMISSIONS", (9, "CONTEXT") => "DEFAULT ASSISTANT CONTEXT",
                (12, "EVERY") => "SYNC FREQUENCY", (12, "KEY") => "ENCRYPTION KEY", (12, "CARRIERS") => "SYNC DESTINATIONS",
                (13, "PRIVATE") => "PROFILE STORAGE", _ => &title,
            }.to_string();
            match control {
                Control::Choice(options) => {
                    // Split mixed rows into independent setting groups. A toggle's
                    // hit used to mean 'invert'; cards now always name a value.
                    let mut groups: Vec<Vec<(String, Hit, bool)>> = Vec::new();
                    for option in options {
                        let independent = matches!(option.1, Hit::PortsShow(..) | Hit::AskCtx(_)) || is_action(option.1);
                        if groups.last().is_some_and(|g| (is_action(g[0].1) && is_action(option.1)) || (!independent && std::mem::discriminant(&g[0].1) == std::mem::discriminant(&option.1))) {
                            groups.last_mut().unwrap().push(option);
                        } else { groups.push(vec![option]); }
                    }
                    for group in groups {
                        let (name, hit, selected) = &group[0];
                        // Options the real renderers can draw are chosen by
                        // picture, on live pages too (real_pics.rs).
                        if live && !is_action(*hit) && group.iter().all(|o| crate::real_pics::real(o.1)) {
                            out.push((title.clone(), Control::Pics(group.into_iter().map(|(n,h,on)| (n, description(h).unwrap_or_else(|| self.setting_label(h)), Pic::Setting(h), h, on)).collect())));
                            continue;
                        }
                        if live {
                            // Live pages: the choice stays one row of chips, and
                            // a line under it says what the current one does.
                            if let Some((label, yes, no, value)) = switch(*hit).filter(|_| !is_action(*hit) && group.len() == 1) {
                                let current = if group.len() == 1 { *selected } else { group.iter().find(|o| o.1 == yes).map(|o| o.2).unwrap_or(!value) };
                                out.push((label.into(), Control::Choice(vec![("ON".into(), yes, current), ("OFF".into(), no, !current)])));
                                if let Some(d) = description(if current { yes } else { no }) { out.push((String::new(), Control::Help(d))); }
                            } else {
                                let row_title = match hit { Hit::JournalKeep(_) => "KEEP HISTORY FOR".into(), Hit::FoldOver(_) => "FOLD LONG OUTPUT".into(), _ => title.clone() };
                                let picked = group.iter().find(|o| o.2).map(|o| o.1);
                                let _ = name;
                                out.push((row_title, Control::Choice(group)));
                                if let Some(d) = picked.and_then(description) { out.push((String::new(), Control::Help(d))); }
                            }
                            continue;
                        }
                        if is_action(*hit) {
                            out.push((title.clone(), Control::Actions(group.into_iter().map(|(n,h,_)| (n, description(h).unwrap_or_else(|| self.setting_label(h)), action_icon(h), h)).collect())));
                        } else if let Some((label, yes, no, value)) = switch(*hit) {
                            let current = if group.len() == 1 { *selected } else { group.iter().find(|o| o.1 == yes).map(|o| o.2).unwrap_or(!value) };
                            out.push((label.into(), Control::Pics(vec![
                                ("ON".into(), description(yes).unwrap_or_else(|| self.setting_label(yes)), Pic::Setting(yes), yes, current),
                                ("OFF".into(), description(no).unwrap_or_else(|| self.setting_label(no)), Pic::Setting(no), no, !current),
                            ])));
                        } else if let Hit::PortsShow(k, _) = hit {
                            let k = *k;
                            out.push((["SYSTEM SERVICES", "UDP LISTENERS", "ACTIVE CONNECTIONS", "DOCKER PORTS"][k.min(3) as usize].into(), Control::Pics([true,false].into_iter().map(|v| {
                                let h = Hit::PortsShow(k,v); ((if v { "SHOW" } else { "HIDE" }).into(), description(h).unwrap(), Pic::Setting(h), h, *selected == v)
                            }).collect())));
                        } else {
                            let row_title = match hit { Hit::AskCtx(c)=>format!("INCLUDE {}", c.key().to_uppercase()), Hit::FoldOver(_)=>"FOLD LONG OUTPUT".into(), Hit::JournalKeep(_)=>"KEEP HISTORY FOR".into(), _=>title.clone() };
                            let _ = name;
                            out.push((row_title, Control::Pics(group.into_iter().map(|(n,h,on)| {
                                let caption = description(h).unwrap_or_else(|| self.setting_label(h));
                                let n = if matches!(h, Hit::AskCtx(_)) { if on { "INCLUDED · REMOVE" } else { "EXCLUDED · ADD" }.into() } else { n };
                                (n, caption, Pic::Setting(h), h, on)
                            }).collect())));
                        }
                    }
                }
                Control::Buttons(items) if live => out.push((title, Control::Buttons(items))),
                Control::Buttons(items) => out.push((title, Control::Actions(items.into_iter().map(|(n,icon,h)| (n, description(h).unwrap_or_else(|| self.setting_label(h)), icon,h)).collect()))),
                other => out.push((title, other)),
            }
        }
        out
    }

    pub(super) fn draw_setting_picture(&mut self, scene: &mut Scene, r: Rect, hit: Hit) {
        let ink = self.theme.ink;
        let sig = self.surface.signal;
        let paper = self.theme.paper;
        let faint = fade(ink, 0.22);
        let line = self.px(1.5);
        let window = |s: &mut Scene, b: Rect| { s.outline(b,line,ink); s.hline(b.x,b.y+b.h*0.18,b.w,line,faint); };
        let rows = |s: &mut Scene, b: Rect, n: usize, colour| { for i in 0..n { s.hline(b.x,b.y+i as f32*b.h/n as f32,b.w*[0.85,0.6,1.0][i%3],line*1.4,colour); } };
        let is_on = switch(hit).map(|s| s.3).unwrap_or(true);
        let accent = if is_on { sig } else { faint };
        match hit {
            Hit::MenuEnabled(enabled)=>{
                window(scene,r);rows(scene,Rect::new(r.x+r.w*0.12,r.y+r.h*0.34,r.w*0.45,r.h*0.35),3,faint);
                if let Some(bind)=self.desktop_icon(){let side=self.px(16.0).min(r.h*0.2);if enabled{scene.texture(Rect::new(r.right()-side*1.6,r.y+(r.h*0.18-side)/2.0,side,side),bind.clone(),None);}scene.texture(Rect::new(r.x+r.w*0.1,r.bottom()-side*1.4,side,side),bind,None);}
            }
            Hit::MenuNames(names)=>{
                window(scene,r);let words=if names{"Project build"}else{"Task 1"};let style=Style{px:self.px(10.0),..self.label()};let words=self.fit(style,words,r.w*0.8);self.fonts.draw(scene,style,r.x+r.w*0.1,r.y+r.h*0.5,&words);scene.hline(r.x+r.w*0.1,r.y+r.h*0.7,r.w*0.6,line*2.0,sig);
            }
            Hit::MenuRecent(recent)=>{
                window(scene,r);let style=Style{px:self.px(10.0),..self.label()};self.fonts.draw(scene,style,r.x+r.w*0.1,r.y+r.h*0.5,"Running");if recent{self.fonts.draw(scene,Style{color:self.theme.dim,..style},r.x+r.w*0.1,r.y+r.h*0.84,"Finished");}
            }
            Hit::MenuSignal(style)=>{
                scene.hline(r.x,r.y+r.h*0.2,r.w,line,ink);let x=r.x+r.w*0.3;let y=r.y+r.h*0.3;
                if let Some(bind)=self.desktop_icon(){scene.texture(Rect::new(x,y,self.px(28.0),self.px(28.0)),bind,None);}
                match style {crate::menu_drawer::SignalStyle::Dot=>scene.push(nus_render::Instance::rounded(Rect::new(x+self.px(24.0),y+self.px(17.0),self.px(7.0),self.px(7.0)),self.px(3.5),sig)),other=>{self.fonts.draw(scene,Style{px:self.px(11.0),..self.label()},x+self.px(28.0),y+self.px(21.0),if other==crate::menu_drawer::SignalStyle::Count{"2"}else{"2 active"});}}
            }
            Hit::MenuDensity(module,density)=>{
                scene.outline(r,line,faint);let inner=r.inset(self.px(6.0));
                if density==crate::menu_drawer::Density::Hidden{self.fonts.draw_icon(scene,icons::CLOSE,self.px(20.0),r.x+r.w*0.4,r.y+r.h*0.3,faint);}
                else if density==crate::menu_drawer::Density::Expanded{for i in 0..if module==crate::menu_drawer::Module::Shortcuts{4}else{2}{let tiles=module==crate::menu_drawer::Module::Shortcuts;let b=Rect::new(inner.x+if tiles{(i%2)as f32*inner.w*0.52}else{0.0},inner.y+if tiles{(i/2)as f32*inner.h*0.52}else{i as f32*inner.h*0.52},inner.w*if tiles{0.46}else{1.0},inner.h*0.42);scene.rect(b,fade(sig,0.13));scene.outline(b,line,ink);rows(scene,b.inset(self.px(5.0)),2,sig);}}
                else{rows(scene,inner,3,ink);}
            }
            Hit::Links(Links::Split) | Hit::PromptUrl(PromptUrl::Split) | Hit::PortsOpen(PortsOpen::Split) => self.draw_pic(scene,r,Pic::StartLayout),
            Hit::Links(Links::NewTab) | Hit::PromptUrl(PromptUrl::NewTab) | Hit::PortsOpen(PortsOpen::Tab) => self.draw_pic(scene,r,Pic::LeadBrowser),
            Hit::DefaultProfile(_) => self.draw_pic(scene,r,Pic::LeadTerminal),
            Hit::ScrollEasing(easing) => {
                scene.hline(r.x,r.bottom(),r.w,line,faint);scene.vline(r.x,r.y,r.h,line,faint);
                for i in 0..40 {
                    let x=i as f32/40.0;let next=(i+1) as f32/40.0;
                    let y=r.bottom()-r.h*easing.at(x);let ny=r.bottom()-r.h*easing.at(next);
                    scene.poly(&[[r.x+r.w*x,y],[r.x+r.w*next,ny],[r.x+r.w*next,ny+line*2.0],[r.x+r.w*x,y+line*2.0]],sig);
                }
            }
            Hit::WheelLines(n) | Hit::FoldOver(n) | Hit::JournalKeep(n) => {
                window(scene,r);rows(scene,Rect::new(r.x+self.px(8.0),r.y+r.h*0.3,r.w*0.45,r.h*0.5),4,faint);
                let st=Style{px:self.px(24.0),color:sig,..self.label()};let text=if n==0 {"OFF".into()} else {n.to_string()};
                self.fonts.draw(scene,st,r.x+r.w*0.6,r.y+r.h*0.7,&text);
            }
            Hit::HatchHotkey(chord) => {
                scene.outline(Rect::new(r.x,r.y+r.h*0.25,r.w,r.h*0.5),line,ink);
                let st=Style{px:self.px(10.0),..self.label()};let text=self.fit(st,chord.label(),r.w-self.px(10.0));
                self.fonts.draw(scene,st,r.x+self.px(5.0),r.y+r.h*0.57,&text);
            }
            Hit::PinDisplay(mode) => self.draw_setting_picture(scene, r, Hit::SmallTabs(match mode {
                crate::pins::Display::Icon => crate::sidebar::SmallTabs::Favicons,
                crate::pins::Display::Preview => crate::sidebar::SmallTabs::Preview,
            })),
            Hit::SmallTabs(mode) => {
                window(scene,r);let column=Rect::new(r.x,r.y+r.h*0.18,r.w*0.27,r.h*0.82);scene.rect(column,fade(sig,0.14));scene.vline(column.right(),column.y,column.h,line,ink);
                for i in 0..3 {let y=column.y+self.px(5.0)+i as f32*column.h*0.28;let size=(column.w-self.px(10.0)).min(self.px(14.0));let x=column.x+(column.w-size)*0.5;
                    if mode==crate::sidebar::SmallTabs::Preview {let p=Rect::new(x-self.px(2.0),y,size+self.px(4.0),size*0.64);scene.rect(p,paper);scene.outline(p,line,sig);scene.hline(p.x,p.y+p.h*0.25,p.w,line,faint);}
                    else {self.fonts.draw_icon(scene,if mode==crate::sidebar::SmallTabs::Icons{icons::GLOBE}else{[icons::GLOBE,icons::CODE,icons::BOOK][i]},size,x,y,if mode==crate::sidebar::SmallTabs::Icons{ink}else{sig});}
                }
            }
            Hit::DownloadRename(mode) => {
                window(scene,r);let file=Rect::new(r.x+r.w*0.12,r.y+r.h*0.3,r.w*0.7,r.h*0.46);scene.outline(file,line,ink);
                self.fonts.draw_icon(scene,icons::DOWNLOAD,self.px(14.0),file.x+self.px(6.0),file.y+self.px(5.0),sig);
                let st=Style{px:self.px(9.0),..self.label()};let word=if mode==crate::downloads::Rename::Off{"file_042.pdf"}else{"Page title.pdf"};self.fonts.draw(scene,st,file.x+self.px(6.0),file.bottom()-self.px(6.0),word);
            }
            Hit::HdrStyle(_) | Hit::HdrMasthead(_) | Hit::HdrDateline(_) | Hit::HdrButton(_) | Hit::HdrNextRow(_) |
            Hit::HdrName(_) | Hit::HdrCaret(_) | Hit::HdrRailHover(_) | Hit::HdrFlash(_) | Hit::Compact(_) |
            Hit::Side(_) | Hit::Pin(_) | Hit::HoverFrom(_) | Hit::Fullscreen(_) => {
                window(scene,r);
                let right = matches!(hit, Hit::Side(Side::Right));
                let compact = matches!(hit, Hit::Compact(true));
                let w = r.w * if compact {0.15} else {0.36};
                let sx = if right {r.right()-w} else {r.x};
                let b = Rect::new(sx,r.y+r.h*0.18,w,r.h*0.82);
                let hidden = matches!(hit, Hit::Fullscreen(Fullscreen::Hidden) | Hit::Pin(false) | Hit::HdrRailHover(true));
                if !hidden {
                    scene.rect(b,fade(sig,0.18)); scene.outline(b,line,faint);
                    if matches!(hit, Hit::HdrStyle(HeaderStyle::Rail)) { for i in 0..3 { scene.rect(Rect::new(sx+self.px(3.0),b.y+self.px(6.0)+i as f32*self.px(12.0),self.px(6.0),self.px(6.0)),sig); } }
                    else {
                        let title_h = if matches!(hit, Hit::HdrMasthead(true)) { self.px(7.0) } else {self.px(3.0)};
                        scene.rect(Rect::new(sx+self.px(5.0),b.y+self.px(6.0),w-self.px(10.0),title_h),ink);
                        for i in 0..3 { let y=b.y+self.px(21.0)+i as f32*self.px(10.0); scene.rect(Rect::new(sx+self.px(5.0),y,self.px(4.0),self.px(4.0)),if i==0 {sig} else {faint}); if !compact { scene.hline(sx+self.px(13.0),y+self.px(2.0),(w-self.px(18.0)).max(1.0),line,faint); } }
                    }
                } else { scene.rect(Rect::new(sx,r.y+r.h*0.2,self.px(3.0),r.h*0.7),sig); }
                let plus_y = if matches!(hit, Hit::HdrNextRow(_)) { b.bottom()-self.px(7.0) } else {r.y+self.px(6.0)};
                if matches!(hit, Hit::HdrButton(true)|Hit::HdrNextRow(true)|Hit::HdrCaret(true)) { scene.hline(sx+w-self.px(12.0),plus_y,self.px(7.0),line,sig); scene.vline(sx+w-self.px(8.5),plus_y-self.px(3.5),self.px(7.0),line,sig); }
                if matches!(hit,Hit::HdrDateline(true)|Hit::HdrName(true)) { scene.hline(sx+self.px(5.0),b.y+self.px(16.0),w*0.75,line,sig); }
            }
            Hit::Links(Links::Stack) | Hit::PaneDivider(_) | Hit::PaneControls(_) | Hit::OpenedBy(_) | Hit::Dedupe(_) => {
                let split = matches!(hit,Hit::Links(Links::Split)|Hit::PromptUrl(PromptUrl::Split)|Hit::PaneDivider(_)|Hit::PaneControls(_));
                window(scene,r);
                if split { scene.vline(r.x+r.w*0.5,r.y+r.h*0.18,r.h*0.82,line,accent); }
                else { scene.rect(Rect::new(r.x+self.px(4.0),r.y+self.px(3.0),r.w*0.25,self.px(6.0)),ink); scene.rect(Rect::new(r.x+r.w*0.33,r.y+self.px(3.0),r.w*0.25,self.px(6.0)),sig); }
                rows(scene,Rect::new(r.x+r.w*0.07,r.y+r.h*0.35,r.w*0.34,r.h*0.45),4,faint);
                let dest = Rect::new(r.x+r.w*0.57,r.y+r.h*0.29,r.w*0.34,r.h*0.58);
                scene.rect(dest,fade(accent,0.22)); rows(scene,dest.inset(self.px(5.0)),3,accent);
                if matches!(hit,Hit::Links(Links::Stack)) { scene.vline(r.x+r.w*0.45,r.y+r.h*0.3,r.h*0.4,line,sig); scene.hline(r.x+r.w*0.45,r.y+r.h*0.7,r.w*0.12,line,sig); }
            }
            Hit::HatchLook(_) | Hit::HatchSize(_) | Hit::HatchMonitor(_) | Hit::HatchSpaces(_) | Hit::HatchAutohide(_) => {
                scene.outline(r,line,faint);
                let h = match hit {Hit::HatchSize(n)=>n as f32/100.0, _=>0.6};
                let floating=matches!(hit,Hit::HatchLook(HatchLook::Card));
                let b=Rect::new(r.x+r.w*0.12,r.y+if floating {r.h*0.2} else {0.0},r.w*0.76,r.h*h);
                scene.rect(b,ink); rows(scene,b.inset(self.px(7.0)),3,paper); scene.rect(Rect::new(b.x+self.px(7.0),b.bottom()-self.px(7.0),self.px(5.0),self.px(5.0)),accent);
            }
            Hit::PortsGrouping(_) | Hit::PortsOpen(_) | Hit::PortsPoll(_) | Hit::PortsToast(_) | Hit::PortsShow(..) | Hit::PortsKill(_) | Hit::PortsProbe(_) | Hit::PortsTunnel(_) | Hit::PortsRemember(_) => {
                window(scene,r);
                for (i,port) in ["3000","5173","8080"].iter().enumerate() {
                    let y=r.y+r.h*0.33+i as f32*r.h*0.23;
                    scene.rect(Rect::new(r.x+self.px(6.0),y-self.px(5.0),self.px(5.0),self.px(5.0)),accent);
                    self.fonts.draw(scene,Style {px:self.px(10.0),..self.label()},r.x+self.px(18.0),y,port);
                    scene.hline(r.x+r.w*0.58,y-self.px(3.0),r.w*0.32,line,faint);
                }
                if matches!(hit,Hit::PortsShow(_,false)) { scene.hline(r.x+self.px(4.0),r.y+r.h*0.56,r.w-self.px(8.0),line,ink); }
            }
            Hit::SyncSession(_) | Hit::SyncEvery(_) | Hit::SyncAtQuit(_) | Hit::Phone(_) => {
                let a=Rect::new(r.x,r.y+r.h*0.22,r.w*0.34,r.h*0.55); let b=Rect::new(r.x+r.w*0.7,r.y+r.h*0.13,r.w*0.28,r.h*0.72);
                window(scene,a); window(scene,b); rows(scene,a.inset(self.px(6.0)),3,faint); rows(scene,b.inset(self.px(6.0)),3,accent);
                scene.hline(a.right()+self.px(3.0),r.y+r.h*0.5,r.w*0.29,line,accent);
                if is_on { scene.poly(&[[b.x-self.px(4.0),r.y+r.h*0.5],[b.x-self.px(11.0),r.y+r.h*0.5-self.px(4.0)],[b.x-self.px(11.0),r.y+r.h*0.5+self.px(4.0)]],sig); }
            }
            Hit::SleepAfter(_) | Hit::ArchiveAfter(_) | Hit::Replay(_) | Hit::TidyEvery(_) => {
                let b=Rect::new(r.x,r.y,r.w*0.65,r.h*0.7); window(scene,b); rows(scene,b.inset(self.px(9.0)),3,faint);
                let cx=r.x+r.w*0.77;let cy=r.y+r.h*0.65; let d=r.h*0.5;
                scene.rect(Rect::new(cx-d/2.0,cy-d/2.0,d,d),paper);
                self.fonts.draw_icon(scene,icons::HISTORY,d,cx-d/2.0,cy-d/2.0,sig);
            }
            Hit::AskCtx(_) | Hit::AskBackend(_) | Hit::Hands(_) | Hit::HandsSubmit(_) => {
                let b=Rect::new(r.x,r.y,r.w*0.6,r.h*0.8);window(scene,b); rows(scene,b.inset(self.px(9.0)),3,faint);
                let c=Rect::new(r.x+r.w*0.42,r.y+r.h*0.37,r.w*0.58,r.h*0.6);scene.rect(c,paper);scene.outline(c,line,sig);rows(scene,c.inset(self.px(8.0)),3,accent);
                if matches!(hit,Hit::Hands(HandsMode::Never)) {scene.hline(c.x+self.px(5.0),c.y+c.h/2.0,c.w-self.px(10.0),line,ink);}
            }
            Hit::Block(_) | Hit::PageSmooth(_) | Hit::ClickToSource(_) | Hit::BarStyle(_) | Hit::BarColor(_) | Hit::StatusStyle(_) => {
                window(scene,r);rows(scene,Rect::new(r.x+r.w*0.1,r.y+r.h*0.35,r.w*0.8,r.h*0.5),4,faint);
                scene.hline(r.x+line,r.y+r.h*0.2,r.w*0.62,self.px(3.0),accent);
                if matches!(hit,Hit::Block(true)) {scene.rect(Rect::new(r.x+r.w*0.63,r.y+r.h*0.38,r.w*0.25,r.h*0.4),paper);scene.outline(Rect::new(r.x+r.w*0.63,r.y+r.h*0.38,r.w*0.25,r.h*0.4),line,sig);}
            }
            _ => {
                self.draw_pic(scene,r,Pic::LeadTerminal);
                scene.hline(r.x+r.w*0.25,r.y+r.h*0.81,r.w*0.45,self.px(3.0),accent);
            }
        }
    }
}

impl App {
    /// Native regression hook: exercise the same targets exposed by the page,
    /// then check selected states and the serialized preferences after each.
    pub(crate) fn check_settings_bindings(&mut self) {
        self.save_prefs();
        let original = crate::prefs::Prefs::load();
        let mut checked = 0;
        for section in 1..14 {
            if section == 2 { continue; } // Covered by check-startup.py.
            let controls: Vec<_> = self.rows_for(section).into_iter().flat_map(|(_,row)| match row {
                Control::Pics(cards) => cards.into_iter().map(|(_,_,_,h,_)|h).collect(),
                _ => Vec::new(),
            }).collect();
            for hit in controls {
                if matches!(hit, Hit::Phone(_) | Hit::HatchHotkey(_)) || is_action(hit) {continue;}
                let before = self.setting_states(section).iter().find(|(h,_)|*h==hit).map(|(_,on)|*on).unwrap();
                self.apply_setting(hit, 0.0);
                let after = self.setting_states(section).iter().find(|(h,_)|*h==hit).map(|(_,on)|*on).unwrap();
                assert_eq!(after, if matches!(hit, Hit::AskCtx(_)) {!before} else {true}, "selection did not bind: {hit:?}");
                if matches!(hit,Hit::Replay(_)) {assert_eq!(self.recorder.is_some(),self.behavior.replay.days().is_some());}
                self.save_prefs();
                let saved = crate::prefs::Prefs::load();
                assert_eq!(serde_json::to_value(saved.behavior).unwrap(),serde_json::to_value(Some(&self.behavior)).unwrap(),"not saved: {hit:?}");
                assert_eq!(serde_json::to_value(saved.header).unwrap(),serde_json::to_value(Some(&self.header)).unwrap());
                assert_eq!(serde_json::to_value(saved.sidebar).unwrap(),serde_json::to_value(Some(&self.sidebar_rules)).unwrap());
                checked += 1;
            }
        }
        for (slider,min,max) in [(Slider::TexScale,1.0,10.0),(Slider::CurWeight,1.0,6.0),(Slider::Saturation,0.5,1.5)] {
            for v in [0.0,0.5,1.0] {
                self.set_slider(slider,v);
                let actual=match slider {Slider::TexScale=>self.surface.texture_scale,Slider::CurWeight=>self.cursor.weight,_=>self.theme_edit.saturation};
                assert!((actual-(min+v*(max-min))).abs()<0.01,"slider {slider:?} {v} -> {actual}");
            }
        }
        let changed = self.prefs_baseline.borrow().clone();
        self.apply_prefs(original);
        *self.prefs_baseline.borrow_mut() = changed;
        self.recorder=self.behavior.replay.days().and_then(crate::replay::Recorder::new);
        self.rebuild_theme();self.layout();self.save_prefs();
        eprintln!("SETTINGS CHECK: {checked} choices bound and saved; slider ranges passed");
    }
}

impl App {
    /// App-owned labels must never show a serialized value or an unfilled slot.
    pub(crate) fn check_ui_labels(&self) {
        let mut text=Vec::new();
        for section in 0..14 {for (title,control) in self.rows_for(section) {
            text.push(title);
            match control {
                Control::Info(s)|Control::Slider(_,_,s)|Control::Cue(_,_,s)=>text.push(s),
                Control::Keys(keys,s)=>{text.extend(keys);text.push(s);},
                Control::Choice(v)|Control::Strip(v)=>text.extend(v.into_iter().map(|(s,_,_)|s)),
                Control::Buttons(v)=>text.extend(v.into_iter().map(|(s,_,_)|s)),
                Control::Proof(v)=>text.extend(v.into_iter().map(|(_,s)|s)),
                Control::Tabs(v)=>text.extend(v.into_iter().map(|(_,_,s,_)|s)),
                Control::Cards(v)=>text.extend(v.into_iter().map(|(s,_,_,_,_,_,_)|s)),
                Control::Tokens(v,_)=>{for(s,_,caption,_,_)in v{text.extend([s,caption]);}},
                Control::Art(v)=>{for(_,s,caption,_,_,_)in v{text.extend([s,caption]);}},
                Control::Pics(v)=>{for(s,caption,_,_,_)in v{text.extend([s,caption]);}},
                Control::Actions(v)=>{for(s,caption,_,_)in v{text.extend([s,caption]);}},
                _=>{},
            }
        }}
        for query in ["","home","downloads","assistants","fonts"] {text.extend(self.prompt_rows(query).into_iter().map(|r|r.text));}
        for label in &text {
            assert!(!["null","undefined","[object Object]","{}","[]"].contains(&label.trim()),"uninitialized UI label: {label}");
            for i in 0..10 {assert!(!label.contains(&format!("{{{i}}}")),"unfilled UI label: {label}");}
        }
        eprintln!("UI_LABELS_CHECKED {}",text.len());
    }
}
