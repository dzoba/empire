//! Commands and the five menus, as in the original (Table V of the manual).

use crate::input::{Key, KeyEvent};
use crate::ui::{Menu, MenuItem};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    // File
    NewGame,
    Load,
    Save,
    SaveAs,
    Delete,
    ExitGame,
    MapEditor,
    Quit,
    // Reports
    WorldMap,
    Status,
    CombatReport,
    Info,
    ShipReport,
    SetProduction,
    ProductionMap,
    // Orders
    GoHome,
    GoDirection,
    GoRandom,
    MoveTo,
    PatrolTo,
    EscortShip,
    Sentry,
    ClearOrders,
    LoadShip,
    UnloadShip,
    SkipMove,
    // Commands
    MoveMode,
    SurveyMode,
    GroupSurvey,
    Wait,
    FlightPaths,
    Activate,
    CenterScreen,
    CenterCursor,
    /// Skip to the next unit that needs orders.
    NextUnit,
    /// Skip every idle unit and end the turn; standing orders still run.
    EndTurn,
    // Misc
    Sound,
    Display,
    ZoomIn,
    ZoomOut,
    Commanders,
    View,
    About,
    // Map editor
    EdLoadMap,
    EdSaveMap,
    EdSaveMapAs,
    EdExit,
    EdSea,
    EdLand,
    EdCity,
    EdAllSea,
    EdAllLand,
    EdGrowLand,
    EdGrowSea,
    EdBlotchLand,
    EdBlotchSea,
    EdSprinkle,
    EdBuildWorld,
    EdWatchBuild,
    EdValidate,
    EdWorldView,
    // Map scrolling
    ScrollLeft,
    ScrollRight,
    ScrollUp,
    ScrollDown,
    PageLeft,
    PageRight,
    PageUp,
    PageDown,
}

/// What's possible right now, for greying out menu items.
#[derive(Clone, Copy, Debug, Default)]
pub struct CmdContext {
    pub in_game: bool,
    pub game_over: bool,
    pub viewer_human: bool,
    pub has_unit: bool,
    pub unit_is_fighter: bool,
    pub unit_is_ship: bool,
    pub unit_is_army: bool,
    pub unit_carries: bool,
    pub on_own_city: bool,
    pub can_move_unit: bool,
    pub survey: bool,
    pub sound: bool,
    pub editor_available: bool,
    pub editor: bool,
    pub watch_build: bool,
}

impl CmdContext {
    pub fn enabled(&self, c: Cmd) -> bool {
        use Cmd::*;
        let play = self.in_game && self.viewer_human && !self.game_over;
        match c {
            NewGame | Load => !self.in_game || self.game_over,
            Delete => true,
            Save | SaveAs => self.in_game && !self.game_over,
            ExitGame => self.in_game,
            MapEditor => !self.in_game && !self.editor,
            Quit => true,
            WorldMap | Status | CombatReport | ShipReport | Info | ProductionMap => self.in_game,
            SetProduction => play && self.on_own_city,
            GoHome => play && self.has_unit && (self.unit_is_fighter || self.unit_is_ship),
            GoRandom => play && self.has_unit && !self.unit_is_fighter,
            EscortShip => play && self.has_unit && self.unit_is_ship,
            LoadShip | UnloadShip => play && ((self.has_unit && self.unit_carries) || (c == UnloadShip && self.on_own_city)),
            GoDirection | MoveTo | PatrolTo | Sentry => play && self.has_unit,
            ClearOrders => play && (self.has_unit || self.on_own_city),
            SkipMove | Wait => play && self.can_move_unit && !self.survey,
            Activate => play && self.survey && self.has_unit,
            FlightPaths => play && self.on_own_city,
            MoveMode => play && self.survey,
            SurveyMode | GroupSurvey => play,
            CenterScreen | CenterCursor => self.in_game,
            NextUnit => play,
            EndTurn => play && self.can_move_unit,
            ZoomIn | ZoomOut => self.in_game || self.editor,
            Sound | Display | Commanders | About => true,
            View => self.in_game && (self.game_over || !self.viewer_human),
            _ => true,
        }
    }
}

fn item(label: &'static str, shortcut: &'static str, c: Cmd, ctx: &CmdContext) -> MenuItem<Cmd> {
    MenuItem { label, shortcut, command: Some(c), enabled: ctx.enabled(c), checked: false }
}

pub fn menus(ctx: &CmdContext) -> Vec<Menu<Cmd>> {
    use Cmd::*;
    let d = MenuItem::divider;
    let mut sound = item("Sound", "ALT+S", Sound, ctx);
    sound.checked = ctx.sound;
    vec![
        Menu {
            title: "&File",
            items: vec![
                item("&New Game", "CTL+N", NewGame, ctx),
                item("&Load", "CTL+L", Load, ctx),
                item("&Save", "CTL+S", Save, ctx),
                item("Save &As", "CTL+A", SaveAs, ctx),
                item("&Delete", "CTL+D", Delete, ctx),
                item("&Exit Game", "CTL+G", ExitGame, ctx),
                d(),
                item("&Map Editor", "CTL+E", MapEditor, ctx),
                d(),
                item("&Quit", "CTL+Q", Quit, ctx),
            ],
        },
        Menu {
            title: "&Reports",
            items: vec![
                item("&Info", "I", Info, ctx),
                d(),
                item("&Status", "F5", Status, ctx),
                item("&Combat Report", "F6", CombatReport, ctx),
                d(),
                item("&World Map", "F7", WorldMap, ctx),
                item("S&hip Report", "F8", ShipReport, ctx),
                d(),
                item("Set &Production", "F9", SetProduction, ctx),
                item("Production &Map", "F10", ProductionMap, ctx),
            ],
        },
        Menu {
            title: "&Orders",
            items: vec![
                item("Go &Home", "H", GoHome, ctx),
                item("Go &Direction", "D", GoDirection, ctx),
                item("Go &Random", "R", GoRandom, ctx),
                d(),
                item("Move &To", "T", MoveTo, ctx),
                item("&Patrol To", "P", PatrolTo, ctx),
                item("&Escort Ship", "E", EscortShip, ctx),
                d(),
                item("&Sentry", "S", Sentry, ctx),
                item("Clear &Orders", "O", ClearOrders, ctx),
                item("&Load Ship", "L", LoadShip, ctx),
                item("&Unload Ship", "U", UnloadShip, ctx),
                d(),
                item("S&kip Move", "<Spc>", SkipMove, ctx),
            ],
        },
        Menu {
            title: "&Commands",
            items: vec![
                item("&Move Mode", "M", MoveMode, ctx),
                item("Sur&vey Mode", "V", SurveyMode, ctx),
                item("&Group Survey", "G", GroupSurvey, ctx),
                d(),
                item("&Wait", "W", Wait, ctx),
                item("&Flight Paths", "F", FlightPaths, ctx),
                item("&Activate", "A", Activate, ctx),
                d(),
                item("Center &Screen", "C", CenterScreen, ctx),
                item("Center C&ursor", "ALT+C", CenterCursor, ctx),
                d(),
                item("&Next Unit", "TAB", NextUnit, ctx),
                item("&End Turn", "SHF+SPC", EndTurn, ctx),
            ],
        },
        Menu {
            title: "&Misc",
            items: vec![
                sound,
                item("&Display", "ALT+D", Display, ctx),
                item("Map &Zoom In", "]", ZoomIn, ctx),
                item("Map Zoom &Out", "[", ZoomOut, ctx),
                d(),
                item("Commanders &Log", "ALT+L", Commanders, ctx),
                d(),
                item("&View", "ALT+V", View, ctx),
                item("&Info on Empire", "ALT+I", About, ctx),
            ],
        },
    ]
}

/// Keyboard shortcuts. `survey` changes nothing here; the play screen decides
/// what a command applies to.
pub fn shortcut(ev: &KeyEvent) -> Option<Cmd> {
    use Cmd::*;
    if let Some(c) = ev.ctrl_letter() {
        return Some(match c {
            'N' => NewGame,
            'L' => Load,
            'S' => Save,
            'A' => SaveAs,
            'D' => Delete,
            'G' => ExitGame,
            'E' => MapEditor,
            'Q' => Quit,
            _ => return None,
        });
    }
    if let Some(c) = ev.alt_letter() {
        return Some(match c {
            'C' => CenterCursor,
            'S' => Sound,
            'D' => Display,
            'L' => Commanders,
            'V' => View,
            'I' => About,
            _ => return None,
        });
    }
    if let Key::F(n) = ev.key {
        if ev.ctrl || ev.alt {
            return None;
        }
        return Some(match (n, ev.shift) {
            (1, false) => ScrollLeft,
            (2, false) => ScrollRight,
            (3, false) => ScrollUp,
            (4, false) => ScrollDown,
            (1, true) => PageLeft,
            (2, true) => PageRight,
            (3, true) => PageUp,
            (4, true) => PageDown,
            (5, _) => Status,
            (6, _) => CombatReport,
            (7, _) => WorldMap,
            (8, _) => ShipReport,
            (9, _) => SetProduction,
            (10, _) => ProductionMap,
            _ => return None,
        });
    }
    if ev.key == Key::Space && ev.shift && !ev.ctrl && !ev.alt {
        return Some(EndTurn);
    }
    if let Some(z) = zoom_key(ev) {
        return Some(z);
    }
    match ev.key {
        Key::Tab if !ev.ctrl && !ev.alt => return Some(NextUnit),
        Key::Space | Key::Pad(0) | Key::Insert => return Some(SkipMove),
        Key::Pad(5) | Key::Char('5') => return Some(CenterScreen),
        Key::PadStar => return Some(ClearOrders),
        Key::PadMinus => return Some(GoHome),
        Key::PadPlus => return Some(MoveTo),
        Key::PadDot | Key::Delete => return Some(Wait),
        Key::Char('.') => return Some(Wait),
        _ => {}
    }
    Some(match ev.letter()? {
        'A' => Activate,
        'C' => CenterScreen,
        'D' => GoDirection,
        'E' => EscortShip,
        'F' => FlightPaths,
        'G' => GroupSurvey,
        'H' => GoHome,
        'I' => Info,
        'L' => LoadShip,
        'M' => MoveMode,
        'O' => ClearOrders,
        'P' => PatrolTo,
        'R' => GoRandom,
        'S' => Sentry,
        'T' => MoveTo,
        'U' => UnloadShip,
        'V' => SurveyMode,
        'W' => Wait,
        _ => return None,
    })
}

/// `[` and `]` (or `-` and `=`) zoom the map out and in.
fn zoom_key(ev: &KeyEvent) -> Option<Cmd> {
    if ev.ctrl || ev.alt {
        return None;
    }
    match ev.key {
        Key::Char('[') | Key::Char('-') => Some(Cmd::ZoomOut),
        Key::Char(']') | Key::Char('=') | Key::Char('+') => Some(Cmd::ZoomIn),
        _ => None,
    }
}

/// The map editor's four menus (Table VIII of the manual).
pub fn editor_menus(ctx: &CmdContext) -> Vec<Menu<Cmd>> {
    use Cmd::*;
    let d = MenuItem::divider;
    let on = |label: &'static str, shortcut: &'static str, c: Cmd| MenuItem { label, shortcut, command: Some(c), enabled: true, checked: false };
    let mut watch = on("&Watch Build", "A", EdWatchBuild);
    watch.checked = ctx.watch_build;
    vec![
        Menu {
            title: "&File",
            items: vec![
                on("&Load Map", "CTL+L", EdLoadMap),
                on("&Save Map", "CTL+S", EdSaveMap),
                on("Save Map &As", "CTL+A", EdSaveMapAs),
                on("&Delete", "CTL+D", Delete),
                d(),
                on("&Exit Editor", "CTL+E", EdExit),
            ],
        },
        Menu {
            title: "&Pieces",
            items: vec![
                on("&Sea", "S", EdSea),
                on("&Land", "L", EdLand),
                on("&City", "X", EdCity),
                d(),
                on("All S&ea", "ALT+S", EdAllSea),
                on("All L&and", "ALT+L", EdAllLand),
            ],
        },
        Menu {
            title: "&Growths",
            items: vec![
                on("&Grow Land Mass", "G", EdGrowLand),
                on("Grow Sea &Mass", "ALT+G", EdGrowSea),
                on("&Blotch Land", "B", EdBlotchLand),
                on("Blotch S&ea", "ALT+B", EdBlotchSea),
                d(),
                on("Sprinkle &Cities", "K", EdSprinkle),
                on("Build a &World", "W", EdBuildWorld),
                watch,
            ],
        },
        Menu {
            title: "&Other",
            items: vec![
                on("&Validate Cities", "V", EdValidate),
                d(),
                on("Center &Screen", "C", CenterScreen),
                on("Center C&ursor", "ALT+C", CenterCursor),
                on("World Vie&w", "ALT+W", EdWorldView),
                d(),
                on("&Info on Empire", "ALT+I", About),
            ],
        },
    ]
}

pub fn editor_shortcut(ev: &KeyEvent) -> Option<Cmd> {
    use Cmd::*;
    if let Some(z) = zoom_key(ev) {
        return Some(z);
    }
    if let Some(c) = ev.ctrl_letter() {
        return Some(match c {
            'L' => EdLoadMap,
            'S' => EdSaveMap,
            'A' => EdSaveMapAs,
            'D' => Delete,
            'E' => EdExit,
            _ => return None,
        });
    }
    if let Some(c) = ev.alt_letter() {
        return Some(match c {
            'S' => EdAllSea,
            'L' => EdAllLand,
            'G' => EdGrowSea,
            'B' => EdBlotchSea,
            'C' => CenterCursor,
            'W' => EdWorldView,
            'I' => About,
            _ => return None,
        });
    }
    if let Key::F(n) = ev.key {
        return Some(match (n, ev.shift) {
            (1, false) => ScrollLeft,
            (2, false) => ScrollRight,
            (3, false) => ScrollUp,
            (4, false) => ScrollDown,
            (1, true) => PageLeft,
            (2, true) => PageRight,
            (3, true) => PageUp,
            (4, true) => PageDown,
            (7, _) => EdWorldView,
            _ => return None,
        });
    }
    Some(match ev.letter()? {
        'S' => EdSea,
        'L' => EdLand,
        'X' => EdCity,
        'G' => EdGrowLand,
        'B' => EdBlotchLand,
        'K' => EdSprinkle,
        'W' => EdBuildWorld,
        'A' => EdWatchBuild,
        'V' => EdValidate,
        'C' => CenterScreen,
        _ => return None,
    })
}
