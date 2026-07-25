use crate::{WindowId, WindowState};

pub type AppKey = i32;
pub type SpaceId = u64;
pub type ScreenId = u32;

/// One window — or a synthetic entry for an app with no open window — as the
/// resolver sees it. Pure data; assembled by the winsrv/app wiring, never here.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Window {
    pub id: WindowId,
    pub app: AppKey,
    pub app_name: String,
    pub title: String,
    pub state: WindowState,
    pub fullscreen: bool,
    pub space: Option<SpaceId>,
    pub space_visible: bool,
    pub space_ordinal: u32,
    pub screen: ScreenId,
    pub created: u64,
    pub is_main: bool,
    pub windowless: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppScope {
    All,
    ActiveOnly,
    ExceptActive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceScope {
    All,
    VisibleOnly,
    NonVisibleOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenScope {
    All,
    StripScreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Show,
    ShowAtEnd,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    RecentFocus,
    Creation,
    Alphabetical,
    SpaceOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    Windows,
    PerApp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lens {
    pub apps: AppScope,
    pub spaces: SpaceScope,
    pub screens: ScreenScope,
    pub minimized: Disposition,
    pub hidden: Disposition,
    pub fullscreen: Disposition,
    pub windowless: Disposition,
    pub order: Order,
    pub grouping: Grouping,
}

/// Lens 1 (⌘⇥): everything visible, windowless trailing, MRU, one tile per window.
impl Default for Lens {
    fn default() -> Self {
        Self {
            apps: AppScope::All,
            spaces: SpaceScope::All,
            screens: ScreenScope::All,
            minimized: Disposition::Show,
            hidden: Disposition::Show,
            fullscreen: Disposition::Show,
            windowless: Disposition::ShowAtEnd,
            order: Order::RecentFocus,
            grouping: Grouping::Windows,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveCtx {
    pub active_app: AppKey,
    pub strip_screen: ScreenId,
}

#[derive(Clone, Copy)]
enum Partition {
    Primary,
    Deferred,
}

fn passes_scope(w: &Window, lens: &Lens, ctx: ResolveCtx) -> bool {
    let apps = match lens.apps {
        AppScope::All => true,
        AppScope::ActiveOnly => w.app == ctx.active_app,
        AppScope::ExceptActive => w.app != ctx.active_app,
    };
    let spaces = match lens.spaces {
        SpaceScope::All => true,
        SpaceScope::VisibleOnly => w.space_visible,
        SpaceScope::NonVisibleOnly => !w.space_visible,
    };
    let screens = match lens.screens {
        ScreenScope::All => true,
        ScreenScope::StripScreen => w.screen == ctx.strip_screen,
    };
    apps && spaces && screens
}

fn partition(w: &Window, lens: &Lens) -> Option<Partition> {
    let mut applicable = Vec::new();
    if w.state.minimized {
        applicable.push(lens.minimized);
    }
    if w.state.hidden {
        applicable.push(lens.hidden);
    }
    if w.fullscreen {
        applicable.push(lens.fullscreen);
    }
    if w.windowless {
        applicable.push(lens.windowless);
    }
    if applicable.contains(&Disposition::Hide) {
        return None;
    }
    if applicable.contains(&Disposition::ShowAtEnd) {
        return Some(Partition::Deferred);
    }
    Some(Partition::Primary)
}

fn sort_partition(part: &mut [Window], order: Order, input: &[Window]) {
    match order {
        Order::RecentFocus => {
            part.sort_by_key(|w| {
                input
                    .iter()
                    .position(|i| i.id == w.id)
                    .unwrap_or(usize::MAX)
            });
        }
        Order::Creation => part.sort_by_key(|w| w.created),
        Order::Alphabetical => part.sort_by(|a, b| {
            a.app_name
                .to_ascii_lowercase()
                .cmp(&b.app_name.to_ascii_lowercase())
                .then_with(|| {
                    a.title
                        .to_ascii_lowercase()
                        .cmp(&b.title.to_ascii_lowercase())
                })
        }),
        Order::SpaceOrder => part.sort_by_key(|w| w.space_ordinal),
    }
}

/// Filter → disposition → group → order → primary ++ deferred.
/// `windows` must already be in recent-focus order, most-recent first.
pub fn resolve(lens: &Lens, windows: &[Window], ctx: &ResolveCtx) -> Vec<WindowId> {
    let mut kept: Vec<(Window, Partition)> = Vec::new();
    for w in windows {
        if !passes_scope(w, lens, *ctx) {
            continue;
        }
        if let Some(part) = partition(w, lens) {
            kept.push((w.clone(), part));
        }
    }

    let tiles: Vec<(Window, Partition)> = match lens.grouping {
        Grouping::Windows => kept,
        Grouping::PerApp => {
            let mut reps: Vec<(Window, Partition)> = Vec::new();
            for (w, part) in kept {
                if let Some(i) = reps.iter().position(|(r, _)| r.app == w.app) {
                    if w.is_main && !reps[i].0.is_main {
                        reps[i] = (w, part);
                    }
                } else {
                    reps.push((w, part));
                }
            }
            reps
        }
    };

    let mut primary: Vec<Window> = Vec::new();
    let mut deferred: Vec<Window> = Vec::new();
    for (w, part) in tiles {
        match part {
            Partition::Primary => primary.push(w),
            Partition::Deferred => deferred.push(w),
        }
    }

    sort_partition(&mut primary, lens.order, windows);
    sort_partition(&mut deferred, lens.order, windows);

    primary.into_iter().chain(deferred).map(|w| w.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(minimized: bool, hidden: bool) -> WindowState {
        let mut tags: u64 = 0x0300_0001_0008_2401;
        let attrs: u64 = 0x3;
        if minimized {
            tags |= 1 << 60;
        }
        if hidden {
            tags |= 1 << 39;
        }
        WindowState::decode(tags, attrs)
    }

    fn win(id: u32, app: AppKey) -> Window {
        Window {
            id: WindowId(id),
            app,
            app_name: format!("app{app}"),
            title: format!("t{id}"),
            state: state(false, false),
            fullscreen: false,
            space: Some(1),
            space_visible: true,
            space_ordinal: 1,
            screen: 0,
            created: u64::from(id),
            is_main: false,
            windowless: false,
        }
    }

    fn ctx(active: AppKey, screen: ScreenId) -> ResolveCtx {
        ResolveCtx {
            active_app: active,
            strip_screen: screen,
        }
    }

    fn ids(raw: &[u32]) -> Vec<WindowId> {
        raw.iter().copied().map(WindowId).collect()
    }

    #[test]
    fn empty_input_yields_empty() {
        let out = resolve(&Lens::default(), &[], &ctx(1, 0));
        assert!(out.is_empty());
    }

    #[test]
    fn active_only_keeps_active_app() {
        let windows = [win(1, 10), win(2, 20), win(3, 10)];
        let lens = Lens {
            apps: AppScope::ActiveOnly,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &windows, &ctx(10, 0)), ids(&[1, 3]));
    }

    #[test]
    fn except_active_drops_active_app() {
        let windows = [win(1, 10), win(2, 20), win(3, 10)];
        let lens = Lens {
            apps: AppScope::ExceptActive,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &windows, &ctx(10, 0)), ids(&[2]));
    }

    #[test]
    fn visible_only_keeps_visible_spaces() {
        let mut visible = win(1, 1);
        visible.space_visible = true;
        let mut hidden_space = win(2, 1);
        hidden_space.space_visible = false;
        let lens = Lens {
            spaces: SpaceScope::VisibleOnly,
            ..Default::default()
        };
        assert_eq!(
            resolve(&lens, &[visible, hidden_space], &ctx(1, 0)),
            ids(&[1])
        );
    }

    #[test]
    fn non_visible_only_keeps_hidden_spaces() {
        let mut visible = win(1, 1);
        visible.space_visible = true;
        let mut hidden_space = win(2, 1);
        hidden_space.space_visible = false;
        let lens = Lens {
            spaces: SpaceScope::NonVisibleOnly,
            ..Default::default()
        };
        assert_eq!(
            resolve(&lens, &[visible, hidden_space], &ctx(1, 0)),
            ids(&[2])
        );
    }

    #[test]
    fn strip_screen_filters_by_screen() {
        let mut on_strip = win(1, 1);
        on_strip.screen = 0;
        let mut other = win(2, 1);
        other.screen = 1;
        let lens = Lens {
            screens: ScreenScope::StripScreen,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[on_strip, other], &ctx(1, 0)), ids(&[1]));
    }

    #[test]
    fn minimized_hide_drops_minimized() {
        let mut mini = win(1, 1);
        mini.state = state(true, false);
        let normal = win(2, 1);
        let lens = Lens {
            minimized: Disposition::Hide,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[mini, normal], &ctx(1, 0)), ids(&[2]));
    }

    #[test]
    fn minimized_show_at_end_defers_preserving_recency() {
        let mut mini_first = win(1, 1);
        mini_first.state = state(true, false);
        let normal_first = win(2, 1);
        let mut mini_second = win(3, 1);
        mini_second.state = state(true, false);
        let normal_second = win(4, 1);
        let lens = Lens {
            minimized: Disposition::ShowAtEnd,
            ..Default::default()
        };
        assert_eq!(
            resolve(
                &lens,
                &[mini_first, normal_first, mini_second, normal_second],
                &ctx(1, 0)
            ),
            ids(&[2, 4, 1, 3])
        );
    }

    #[test]
    fn hide_beats_show_at_end() {
        let mut both = win(1, 1);
        both.state = state(true, true);
        let normal = win(2, 1);
        let lens = Lens {
            minimized: Disposition::ShowAtEnd,
            hidden: Disposition::Hide,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[both, normal], &ctx(1, 0)), ids(&[2]));
    }

    #[test]
    fn two_show_at_end_categories_defer_once() {
        let mut dual = win(1, 1);
        dual.state = state(true, false);
        dual.fullscreen = true;
        let normal = win(2, 1);
        let lens = Lens {
            minimized: Disposition::ShowAtEnd,
            fullscreen: Disposition::ShowAtEnd,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[dual, normal], &ctx(1, 0)), ids(&[2, 1]));
    }

    #[test]
    fn windowless_show_at_end_trails() {
        let normal = win(1, 1);
        let mut ghost = win(2, 2);
        ghost.windowless = true;
        let lens = Lens {
            windowless: Disposition::ShowAtEnd,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[normal, ghost], &ctx(1, 0)), ids(&[1, 2]));
    }

    #[test]
    fn windowless_hide_drops() {
        let normal = win(1, 1);
        let mut ghost = win(2, 2);
        ghost.windowless = true;
        let lens = Lens {
            windowless: Disposition::Hide,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[normal, ghost], &ctx(1, 0)), ids(&[1]));
    }

    #[test]
    fn recent_focus_preserves_input_order() {
        let windows = [win(3, 1), win(1, 2), win(2, 3)];
        let lens = Lens {
            order: Order::RecentFocus,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &windows, &ctx(1, 0)), ids(&[3, 1, 2]));
    }

    #[test]
    fn creation_sorts_oldest_first() {
        let mut late = win(1, 1);
        late.created = 30;
        let mut early = win(2, 1);
        early.created = 10;
        let mut mid = win(3, 1);
        mid.created = 20;
        let lens = Lens {
            order: Order::Creation,
            ..Default::default()
        };
        assert_eq!(
            resolve(&lens, &[late, early, mid], &ctx(1, 0)),
            ids(&[2, 3, 1])
        );
    }

    #[test]
    fn alphabetical_by_app_then_title_case_insensitive() {
        let mut zebra = win(1, 1);
        zebra.app_name = "Zebra".into();
        zebra.title = "b".into();
        let mut apple_z = win(2, 2);
        apple_z.app_name = "apple".into();
        apple_z.title = "Z".into();
        let mut apple_a = win(3, 2);
        apple_a.app_name = "Apple".into();
        apple_a.title = "a".into();
        let lens = Lens {
            order: Order::Alphabetical,
            ..Default::default()
        };
        assert_eq!(
            resolve(&lens, &[zebra, apple_z, apple_a], &ctx(1, 0)),
            ids(&[3, 2, 1])
        );
    }

    #[test]
    fn space_order_by_ordinal_stable_tiebreak() {
        let mut second_space = win(1, 1);
        second_space.space_ordinal = 2;
        let mut first_early = win(2, 1);
        first_early.space_ordinal = 1;
        let mut first_late = win(3, 1);
        first_late.space_ordinal = 1;
        let lens = Lens {
            order: Order::SpaceOrder,
            ..Default::default()
        };
        assert_eq!(
            resolve(&lens, &[second_space, first_early, first_late], &ctx(1, 0)),
            ids(&[2, 3, 1])
        );
    }

    #[test]
    fn per_app_one_tile_prefers_is_main_else_most_recent() {
        let mut app10_recent = win(1, 10);
        app10_recent.is_main = false;
        let mut app20_main = win(2, 20);
        app20_main.is_main = true;
        let mut app10_main = win(3, 10);
        app10_main.is_main = true;
        let mut app20_other = win(4, 20);
        app20_other.is_main = false;
        let lens = Lens {
            grouping: Grouping::PerApp,
            ..Default::default()
        };
        let out = resolve(
            &lens,
            &[app10_recent, app20_main, app10_main, app20_other],
            &ctx(1, 0),
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out, ids(&[2, 3]));
    }

    #[test]
    fn per_app_falls_back_to_most_recent_when_no_main() {
        let recent = win(1, 10);
        let older = win(2, 10);
        let lens = Lens {
            grouping: Grouping::PerApp,
            ..Default::default()
        };
        assert_eq!(resolve(&lens, &[recent, older], &ctx(1, 0)), ids(&[1]));
    }

    #[test]
    fn default_lens_mixed_fixture_windowless_trailing() {
        let first = win(1, 1);
        let second = win(2, 2);
        let mut ghost_early = win(3, 3);
        ghost_early.windowless = true;
        let fourth = win(4, 4);
        let mut ghost_late = win(5, 5);
        ghost_late.windowless = true;
        assert_eq!(
            resolve(
                &Lens::default(),
                &[first, second, ghost_early, fourth, ghost_late],
                &ctx(1, 0)
            ),
            ids(&[1, 2, 4, 3, 5])
        );
    }

    #[test]
    fn combined_scope_disposition_order_grouping() {
        let mut active_normal = win(1, 10);
        active_normal.created = 5;
        active_normal.screen = 0;
        let mut other_mini_main = win(2, 20);
        other_mini_main.created = 1;
        other_mini_main.screen = 0;
        other_mini_main.state = state(true, false);
        other_mini_main.is_main = true;
        let mut active_wrong_screen = win(3, 10);
        active_wrong_screen.screen = 1;
        let mut other_normal = win(4, 20);
        other_normal.created = 2;
        other_normal.screen = 0;
        let mut ghost = win(5, 30);
        ghost.created = 3;
        ghost.screen = 0;
        ghost.windowless = true;

        let lens = Lens {
            apps: AppScope::ExceptActive,
            spaces: SpaceScope::All,
            screens: ScreenScope::StripScreen,
            minimized: Disposition::ShowAtEnd,
            hidden: Disposition::Show,
            fullscreen: Disposition::Show,
            windowless: Disposition::ShowAtEnd,
            order: Order::Creation,
            grouping: Grouping::PerApp,
        };

        assert_eq!(
            resolve(
                &lens,
                &[
                    active_normal,
                    other_mini_main,
                    active_wrong_screen,
                    other_normal,
                    ghost
                ],
                &ctx(10, 0)
            ),
            ids(&[2, 5])
        );
    }

    #[test]
    fn default_lens_is_lens_one() {
        let lens = Lens::default();
        assert_eq!(lens.apps, AppScope::All);
        assert_eq!(lens.spaces, SpaceScope::All);
        assert_eq!(lens.screens, ScreenScope::All);
        assert_eq!(lens.minimized, Disposition::Show);
        assert_eq!(lens.hidden, Disposition::Show);
        assert_eq!(lens.fullscreen, Disposition::Show);
        assert_eq!(lens.windowless, Disposition::ShowAtEnd);
        assert_eq!(lens.order, Order::RecentFocus);
        assert_eq!(lens.grouping, Grouping::Windows);
    }
}
