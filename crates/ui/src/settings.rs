use std::cell::{Cell, OnceCell, RefCell};

use config::{
    Apps, Config, CursorFollowsFocus, Disposition, OnRelease, Order, ShowOn, Size, Style, Theme,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSFont,
    NSPopUpButton, NSSlider, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

const WIN_W: f64 = 560.0;
const WIN_H: f64 = 800.0;
const MARGIN: f64 = 20.0;
const ROW_H: f64 = 24.0;
const GAP: f64 = 8.0;
const SECTION_GAP: f64 = 16.0;
const LABEL_W: f64 = 140.0;
/// Label width inside a half-width lens column — narrower than `LABEL_W` so it
/// cannot run under the control in the column beside it.
const COL_LABEL_W: f64 = 88.0;
/// Space between the two lens columns.
const GUTTER: f64 = 16.0;
const CTRL_X: f64 = MARGIN + LABEL_W + 8.0;
const CTRL_W: f64 = WIN_W - CTRL_X - MARGIN;

struct Widgets {
    summon_slider: Retained<NSSlider>,
    summon_value: Retained<NSTextField>,
    theme: Retained<NSPopUpButton>,
    show_on: Retained<NSPopUpButton>,
    background_capture: Retained<NSButton>,
    start_at_login: Retained<NSButton>,
    menubar_icon: Retained<NSButton>,
    peek_enabled: Retained<NSButton>,
    arrow_keys: Retained<NSButton>,
    vim_keys: Retained<NSButton>,
    hover_select: Retained<NSButton>,
    cursor_follows: Retained<NSPopUpButton>,
    lens_picker: Retained<NSPopUpButton>,
    lens_trigger: Retained<NSTextField>,
    lens_apps: Retained<NSPopUpButton>,
    lens_order: Retained<NSPopUpButton>,
    lens_style: Retained<NSPopUpButton>,
    lens_size: Retained<NSPopUpButton>,
    lens_on_release: Retained<NSPopUpButton>,
    lens_minimized: Retained<NSPopUpButton>,
    lens_hidden: Retained<NSPopUpButton>,
    lens_fullscreen: Retained<NSPopUpButton>,
    lens_windowless: Retained<NSPopUpButton>,
}

struct SettingsIvars {
    config: RefCell<Config>,
    on_change: RefCell<Box<dyn FnMut(Config)>>,
    selected_lens: Cell<usize>,
    suppress: Cell<bool>,
    widgets: OnceCell<Widgets>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SettingsIvars]
    struct SettingsController;

    unsafe impl NSObjectProtocol for SettingsController {}

    impl SettingsController {
        #[unsafe(method(changed:))]
        fn changed(&self, sender: Option<&AnyObject>) {
            if self.ivars().suppress.get() {
                return;
            }
            let Some(widgets) = self.ivars().widgets.get() else {
                return;
            };

            if let Some(sender) = sender {
                let picker: &NSPopUpButton = widgets.lens_picker.as_ref();
                if std::ptr::eq(
                    std::ptr::from_ref(sender),
                    std::ptr::from_ref(picker).cast::<AnyObject>(),
                ) {
                    self.apply_lens_editors_to_config();
                    let idx = widgets.lens_picker.indexOfSelectedItem();
                    if let Ok(i) = usize::try_from(idx) {
                        self.ivars().selected_lens.set(i);
                        self.load_lens_editors();
                    }
                    return;
                }
            }

            self.sync_from_ui();
        }
    }
);

impl SettingsController {
    fn new(
        mtm: MainThreadMarker,
        config: Config,
        on_change: Box<dyn FnMut(Config)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SettingsIvars {
            config: RefCell::new(config),
            on_change: RefCell::new(on_change),
            selected_lens: Cell::new(0),
            suppress: Cell::new(false),
            widgets: OnceCell::new(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn emit(&self) {
        let cfg = self.ivars().config.borrow().clone();
        (self.ivars().on_change.borrow_mut())(cfg);
    }

    fn sync_from_ui(&self) {
        let Some(w) = self.ivars().widgets.get() else {
            return;
        };
        {
            let mut cfg = self.ivars().config.borrow_mut();
            cfg.summon_delay_ms = slider_ms(&w.summon_slider);
            w.summon_value
                .setStringValue(&NSString::from_str(&format!("{} ms", cfg.summon_delay_ms)));
            cfg.theme = popup_theme(&w.theme);
            cfg.show_on = popup_show_on(&w.show_on);
            cfg.background_capture = checkbox_on(&w.background_capture);
            cfg.start_at_login = checkbox_on(&w.start_at_login);
            cfg.menubar_icon = checkbox_on(&w.menubar_icon);
            cfg.peek.enabled = checkbox_on(&w.peek_enabled);
            cfg.controls.arrow_keys = checkbox_on(&w.arrow_keys);
            cfg.controls.vim_keys = checkbox_on(&w.vim_keys);
            cfg.controls.hover_select = checkbox_on(&w.hover_select);
            cfg.controls.cursor_follows_focus = popup_cursor(&w.cursor_follows);
        }
        self.apply_lens_editors_to_config();
        self.emit();
    }

    fn apply_lens_editors_to_config(&self) {
        let Some(w) = self.ivars().widgets.get() else {
            return;
        };
        let idx = self.ivars().selected_lens.get();
        let mut cfg = self.ivars().config.borrow_mut();
        if idx >= cfg.lenses.len() {
            return;
        }
        // A blank field must not discard the rest of the lens edit — keep the
        // previous trigger and apply everything else.
        let typed = w.lens_trigger.stringValue().to_string();
        let trigger = typed.trim();
        let lens = &mut cfg.lenses[idx];
        if !trigger.is_empty() {
            trigger.clone_into(&mut lens.trigger);
        }
        lens.apps = popup_apps(&w.lens_apps);
        lens.order = popup_order(&w.lens_order);
        lens.style = popup_style(&w.lens_style);
        lens.size = popup_size(&w.lens_size);
        lens.on_release = popup_on_release(&w.lens_on_release);
        lens.minimized = popup_disposition(&w.lens_minimized);
        lens.hidden = popup_disposition(&w.lens_hidden);
        lens.fullscreen_windows = popup_disposition(&w.lens_fullscreen);
        lens.windowless_apps = popup_disposition(&w.lens_windowless);
    }

    fn load_lens_editors(&self) {
        let Some(w) = self.ivars().widgets.get() else {
            return;
        };
        self.ivars().suppress.set(true);
        let cfg = self.ivars().config.borrow();
        let idx = self.ivars().selected_lens.get();
        let enabled = !cfg.lenses.is_empty();
        set_enabled_popup(&w.lens_picker, enabled);
        set_enabled_field(&w.lens_trigger, enabled);
        for p in [
            &w.lens_apps,
            &w.lens_order,
            &w.lens_style,
            &w.lens_size,
            &w.lens_on_release,
            &w.lens_minimized,
            &w.lens_hidden,
            &w.lens_fullscreen,
            &w.lens_windowless,
        ] {
            set_enabled_popup(p, enabled);
        }
        if let Some(lens) = cfg.lenses.get(idx) {
            w.lens_trigger
                .setStringValue(&NSString::from_str(&lens.trigger));
            select_title(&w.lens_apps, apps_title(lens.apps));
            select_title(&w.lens_order, order_title(lens.order));
            select_title(&w.lens_style, style_title(lens.style));
            select_title(&w.lens_size, size_title(lens.size));
            select_title(&w.lens_on_release, on_release_title(lens.on_release));
            select_title(&w.lens_minimized, disposition_title(lens.minimized));
            select_title(&w.lens_hidden, disposition_title(lens.hidden));
            select_title(
                &w.lens_fullscreen,
                disposition_title(lens.fullscreen_windows),
            );
            select_title(&w.lens_windowless, disposition_title(lens.windowless_apps));
        } else {
            w.lens_trigger.setStringValue(&NSString::from_str(""));
        }
        self.ivars().suppress.set(false);
    }

    fn rebuild_lens_picker(&self) {
        let Some(w) = self.ivars().widgets.get() else {
            return;
        };
        self.ivars().suppress.set(true);
        w.lens_picker.removeAllItems();
        let cfg = self.ivars().config.borrow();
        for (i, lens) in cfg.lenses.iter().enumerate() {
            let title = format!("{}: {}", i + 1, lens.trigger);
            w.lens_picker.addItemWithTitle(&NSString::from_str(&title));
        }
        if !cfg.lenses.is_empty() {
            let idx = self.ivars().selected_lens.get().min(cfg.lenses.len() - 1);
            self.ivars().selected_lens.set(idx);
            if let Ok(i) = isize::try_from(idx) {
                w.lens_picker.selectItemAtIndex(i);
            }
        }
        drop(cfg);
        self.ivars().suppress.set(false);
        self.load_lens_editors();
    }
}

/// Native settings window: a view over the TOML config. Persistence is the caller's job.
pub struct Settings {
    window: Retained<NSWindow>,
    _controller: Retained<SettingsController>,
}

impl Settings {
    /// `on_change` receives the edited config; the caller persists it.
    pub fn new(
        mtm: MainThreadMarker,
        config: &Config,
        on_change: impl FnMut(Config) + 'static,
    ) -> Self {
        let controller = SettingsController::new(mtm, config.clone(), Box::new(on_change));
        let window = make_window(mtm);
        let content = window.contentView().expect("window content view");
        let widgets = build_widgets(&content, mtm, config, &controller);
        controller
            .ivars()
            .widgets
            .set(widgets)
            .unwrap_or_else(|_| panic!("settings widgets set once"));
        controller.rebuild_lens_picker();
        Self {
            window,
            _controller: controller,
        }
    }

    pub fn show(&self) {
        self.window.makeKeyAndOrderFront(None);
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }
}

fn make_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIN_W, WIN_H)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Oriel Settings"));
    window.center();
    window
}

fn build_widgets(
    content: &NSView,
    mtm: MainThreadMarker,
    config: &Config,
    target: &AnyObject,
) -> Widgets {
    let mut ui = Ui {
        parent: content,
        mtm,
        target,
        y: WIN_H - MARGIN,
    };
    let general = build_general(&mut ui, config);
    let controls = build_controls(&mut ui, config);
    let lenses = build_lenses(&mut ui);
    place_footer(content, mtm, ui.y);
    Widgets {
        summon_slider: general.summon_slider,
        summon_value: general.summon_value,
        theme: general.theme,
        show_on: general.show_on,
        background_capture: general.background_capture,
        start_at_login: general.start_at_login,
        menubar_icon: general.menubar_icon,
        peek_enabled: general.peek_enabled,
        arrow_keys: controls.arrow_keys,
        vim_keys: controls.vim_keys,
        hover_select: controls.hover_select,
        cursor_follows: controls.cursor_follows,
        lens_picker: lenses.picker,
        lens_trigger: lenses.trigger,
        lens_apps: lenses.apps,
        lens_order: lenses.order,
        lens_style: lenses.style,
        lens_size: lenses.size,
        lens_on_release: lenses.on_release,
        lens_minimized: lenses.minimized,
        lens_hidden: lenses.hidden,
        lens_fullscreen: lenses.fullscreen,
        lens_windowless: lenses.windowless,
    }
}

struct Ui<'a> {
    parent: &'a NSView,
    mtm: MainThreadMarker,
    target: &'a AnyObject,
    y: f64,
}

impl Ui<'_> {
    fn section(&mut self, title: &str) {
        self.y = place_section(self.parent, self.mtm, title, self.y);
    }

    fn advance(&mut self) {
        self.y -= ROW_H + GAP;
    }

    fn finish_section(&mut self) {
        // Last control already subtracted ROW_H+GAP; widen to SECTION_GAP.
        self.y -= SECTION_GAP - GAP;
    }

    fn checkbox(&mut self, title: &str, on: bool) -> Retained<NSButton> {
        let b = place_checkbox(self.parent, self.mtm, title, self.y, on, self.target);
        self.advance();
        b
    }

    fn popup(&mut self, label: &str, items: &[&str], selected: &str) -> Retained<NSPopUpButton> {
        let p = place_popup_row(
            self.parent,
            self.mtm,
            label,
            self.y,
            items,
            selected,
            self.target,
        );
        self.advance();
        p
    }

    fn col_popup(
        &self,
        label: &str,
        col: &Col,
        items: &[&str],
        selected: &str,
    ) -> Retained<NSPopUpButton> {
        place_col_popup(
            self.parent,
            self.mtm,
            self.target,
            self.y,
            &Popup {
                label,
                col,
                items,
                selected,
            },
        )
    }
}

struct GeneralControls {
    summon_slider: Retained<NSSlider>,
    summon_value: Retained<NSTextField>,
    theme: Retained<NSPopUpButton>,
    show_on: Retained<NSPopUpButton>,
    background_capture: Retained<NSButton>,
    start_at_login: Retained<NSButton>,
    menubar_icon: Retained<NSButton>,
    peek_enabled: Retained<NSButton>,
}

fn build_general(ui: &mut Ui<'_>, config: &Config) -> GeneralControls {
    ui.section("General");
    let slider = place_slider_row(
        ui.parent,
        ui.mtm,
        "Summon delay",
        ui.y,
        config.summon_delay_ms,
        ui.target,
    );
    ui.advance();
    let theme = ui.popup(
        "Theme",
        &["system", "light", "dark"],
        theme_title(config.theme),
    );
    let show_on = ui.popup(
        "Show on",
        &["active-screen", "pointer-screen", "menubar-screen"],
        show_on_title(config.show_on),
    );
    let background_capture = ui.checkbox("Background capture", config.background_capture);
    let start_at_login = ui.checkbox("Start at login", config.start_at_login);
    let menubar_icon = ui.checkbox("Menu bar icon", config.menubar_icon);
    let peek_enabled = ui.checkbox("Peek enabled", config.peek.enabled);
    ui.finish_section();
    GeneralControls {
        summon_slider: slider.0,
        summon_value: slider.1,
        theme,
        show_on,
        background_capture,
        start_at_login,
        menubar_icon,
        peek_enabled,
    }
}

struct ControlWidgets {
    arrow_keys: Retained<NSButton>,
    vim_keys: Retained<NSButton>,
    hover_select: Retained<NSButton>,
    cursor_follows: Retained<NSPopUpButton>,
}

fn build_controls(ui: &mut Ui<'_>, config: &Config) -> ControlWidgets {
    ui.section("Controls");
    let arrow_keys = ui.checkbox("Arrow keys", config.controls.arrow_keys);
    let vim_keys = ui.checkbox("Vim keys", config.controls.vim_keys);
    let hover_select = ui.checkbox("Hover select", config.controls.hover_select);
    let cursor_follows = ui.popup(
        "Cursor follows focus",
        &["never", "always", "other-screen"],
        cursor_title(config.controls.cursor_follows_focus),
    );
    ui.finish_section();
    ControlWidgets {
        arrow_keys,
        vim_keys,
        hover_select,
        cursor_follows,
    }
}

struct LensControls {
    picker: Retained<NSPopUpButton>,
    trigger: Retained<NSTextField>,
    apps: Retained<NSPopUpButton>,
    order: Retained<NSPopUpButton>,
    style: Retained<NSPopUpButton>,
    size: Retained<NSPopUpButton>,
    on_release: Retained<NSPopUpButton>,
    minimized: Retained<NSPopUpButton>,
    hidden: Retained<NSPopUpButton>,
    fullscreen: Retained<NSPopUpButton>,
    windowless: Retained<NSPopUpButton>,
}

fn build_lenses(ui: &mut Ui<'_>) -> LensControls {
    ui.section("Lenses");
    let picker = ui.popup("Lens", &[], "");
    let trigger = place_text_row(ui.parent, ui.mtm, "Trigger", ui.y, "", ui.target);
    ui.advance();

    let content_w = WIN_W - 2.0 * MARGIN;
    let col_span = (content_w - GUTTER) / 2.0;
    let ctrl_w = col_span - COL_LABEL_W - 8.0;
    let left = Col {
        label_x: MARGIN,
        label_w: COL_LABEL_W,
        ctrl_x: MARGIN + COL_LABEL_W + 8.0,
        ctrl_w,
    };
    let right = Col {
        label_x: MARGIN + col_span + GUTTER,
        label_w: COL_LABEL_W,
        ctrl_x: MARGIN + col_span + GUTTER + COL_LABEL_W + 8.0,
        ctrl_w,
    };

    let apps = ui.col_popup("Apps", &left, &["all", "active", "inactive"], "all");
    let order = ui.col_popup(
        "Order",
        &right,
        &["recent", "created", "alphabetical", "space"],
        "recent",
    );
    ui.advance();
    let style = ui.col_popup("Style", &left, &["gallery", "icons", "list"], "gallery");
    let size = ui.col_popup(
        "Size",
        &right,
        &["small", "medium", "large", "auto"],
        "auto",
    );
    ui.advance();
    let on_release = ui.popup("On release", &["jump", "linger", "filter"], "jump");
    let minimized = ui.col_popup("Minimized", &left, &["show", "end", "hide"], "show");
    let hidden = ui.col_popup("Hidden", &right, &["show", "end", "hide"], "show");
    ui.advance();
    let fullscreen = ui.col_popup("Fullscreen", &left, &["show", "end", "hide"], "show");
    let windowless = ui.col_popup("Windowless", &right, &["show", "end", "hide"], "end");
    ui.y -= ROW_H + SECTION_GAP;

    LensControls {
        picker,
        trigger,
        apps,
        order,
        style,
        size,
        on_release,
        minimized,
        hidden,
        fullscreen,
        windowless,
    }
}

fn place_footer(content: &NSView, mtm: MainThreadMarker, y: f64) {
    let footer = NSTextField::labelWithString(
        &NSString::from_str("~/.config/oriel/config.toml remains the source of truth."),
        mtm,
    );
    footer.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    footer.setFrame(NSRect::new(
        NSPoint::new(MARGIN, y - ROW_H),
        NSSize::new(WIN_W - 2.0 * MARGIN, ROW_H),
    ));
    content.addSubview(&footer);
}

struct Col {
    label_x: f64,
    label_w: f64,
    ctrl_x: f64,
    ctrl_w: f64,
}

struct Popup<'a> {
    label: &'a str,
    col: &'a Col,
    items: &'a [&'a str],
    selected: &'a str,
}

fn place_section(parent: &NSView, mtm: MainThreadMarker, title: &str, y: f64) -> f64 {
    let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    label.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    let top = y - ROW_H;
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, top),
        NSSize::new(WIN_W - 2.0 * MARGIN, ROW_H),
    ));
    parent.addSubview(&label);
    top - GAP
}

fn place_label(parent: &NSView, mtm: MainThreadMarker, text: &str, x: f64, y: f64, w: f64) {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, ROW_H)));
    parent.addSubview(&label);
}

fn place_slider_row(
    parent: &NSView,
    mtm: MainThreadMarker,
    label: &str,
    y: f64,
    value: u32,
    target: &AnyObject,
) -> (Retained<NSSlider>, Retained<NSTextField>) {
    let top = y - ROW_H;
    place_label(parent, mtm, label, MARGIN, top, LABEL_W);
    let slider = NSSlider::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(CTRL_X, top), NSSize::new(CTRL_W - 64.0, ROW_H)),
    );
    slider.setMinValue(0.0);
    slider.setMaxValue(900.0);
    slider.setDoubleValue(f64::from(value));
    slider.setContinuous(true);
    unsafe {
        slider.setTarget(Some(target));
        slider.setAction(Some(sel!(changed:)));
    }
    parent.addSubview(&slider);

    let value_label =
        NSTextField::labelWithString(&NSString::from_str(&format!("{value} ms")), mtm);
    value_label.setFrame(NSRect::new(
        NSPoint::new(CTRL_X + CTRL_W - 60.0, top),
        NSSize::new(60.0, ROW_H),
    ));
    parent.addSubview(&value_label);
    (slider, value_label)
}

fn place_checkbox(
    parent: &NSView,
    mtm: MainThreadMarker,
    title: &str,
    y: f64,
    on: bool,
    target: &AnyObject,
) -> Retained<NSButton> {
    let top = y - ROW_H;
    let button = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(sel!(changed:)),
            mtm,
        )
    };
    button.setFrame(NSRect::new(
        NSPoint::new(MARGIN, top),
        NSSize::new(WIN_W - 2.0 * MARGIN, ROW_H),
    ));
    button.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    parent.addSubview(&button);
    button
}

fn place_popup_row(
    parent: &NSView,
    mtm: MainThreadMarker,
    label: &str,
    y: f64,
    items: &[&str],
    selected: &str,
    target: &AnyObject,
) -> Retained<NSPopUpButton> {
    place_col_popup(
        parent,
        mtm,
        target,
        y,
        &Popup {
            label,
            col: &Col {
                label_x: MARGIN,
                label_w: LABEL_W,
                ctrl_x: CTRL_X,
                ctrl_w: CTRL_W,
            },
            items,
            selected,
        },
    )
}

fn place_col_popup(
    parent: &NSView,
    mtm: MainThreadMarker,
    target: &AnyObject,
    y: f64,
    popup: &Popup<'_>,
) -> Retained<NSPopUpButton> {
    let top = y - ROW_H;
    place_label(
        parent,
        mtm,
        popup.label,
        popup.col.label_x,
        top,
        popup.col.label_w,
    );
    let control = NSPopUpButton::initWithFrame_pullsDown(
        mtm.alloc(),
        NSRect::new(
            NSPoint::new(popup.col.ctrl_x, top),
            NSSize::new(popup.col.ctrl_w, ROW_H),
        ),
        false,
    );
    for item in popup.items {
        control.addItemWithTitle(&NSString::from_str(item));
    }
    if !popup.selected.is_empty() && !popup.items.is_empty() {
        select_title(&control, popup.selected);
    }
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(sel!(changed:)));
    }
    parent.addSubview(&control);
    control
}

fn place_text_row(
    parent: &NSView,
    mtm: MainThreadMarker,
    label: &str,
    y: f64,
    value: &str,
    target: &AnyObject,
) -> Retained<NSTextField> {
    let top = y - ROW_H;
    place_label(parent, mtm, label, MARGIN, top, LABEL_W);
    let field = NSTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(CTRL_X, top), NSSize::new(CTRL_W, ROW_H)),
    );
    field.setStringValue(&NSString::from_str(value));
    unsafe {
        field.setTarget(Some(target));
        field.setAction(Some(sel!(changed:)));
    }
    parent.addSubview(&field);
    field
}

fn slider_ms(slider: &NSSlider) -> u32 {
    let raw = slider.doubleValue().round().clamp(0.0, 900.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        raw as u32
    }
}

fn checkbox_on(button: &NSButton) -> bool {
    button.state() == NSControlStateValueOn
}

fn select_title(popup: &NSPopUpButton, title: &str) {
    popup.selectItemWithTitle(&NSString::from_str(title));
}

fn set_enabled_popup(popup: &NSPopUpButton, enabled: bool) {
    popup.setEnabled(enabled);
}

fn set_enabled_field(field: &NSTextField, enabled: bool) {
    field.setEnabled(enabled);
    field.setEditable(enabled);
}

fn popup_title(popup: &NSPopUpButton) -> String {
    popup
        .titleOfSelectedItem()
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn theme_title(t: Theme) -> &'static str {
    match t {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

fn popup_theme(popup: &NSPopUpButton) -> Theme {
    match popup_title(popup).as_str() {
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => Theme::System,
    }
}

fn show_on_title(s: ShowOn) -> &'static str {
    match s {
        ShowOn::ActiveScreen => "active-screen",
        ShowOn::PointerScreen => "pointer-screen",
        ShowOn::MenubarScreen => "menubar-screen",
    }
}

fn popup_show_on(popup: &NSPopUpButton) -> ShowOn {
    match popup_title(popup).as_str() {
        "pointer-screen" => ShowOn::PointerScreen,
        "menubar-screen" => ShowOn::MenubarScreen,
        _ => ShowOn::ActiveScreen,
    }
}

fn cursor_title(c: CursorFollowsFocus) -> &'static str {
    match c {
        CursorFollowsFocus::Never => "never",
        CursorFollowsFocus::Always => "always",
        CursorFollowsFocus::OtherScreen => "other-screen",
    }
}

fn popup_cursor(popup: &NSPopUpButton) -> CursorFollowsFocus {
    match popup_title(popup).as_str() {
        "always" => CursorFollowsFocus::Always,
        "other-screen" => CursorFollowsFocus::OtherScreen,
        _ => CursorFollowsFocus::Never,
    }
}

fn apps_title(a: Apps) -> &'static str {
    match a {
        Apps::All => "all",
        Apps::Active => "active",
        Apps::Inactive => "inactive",
    }
}

fn popup_apps(popup: &NSPopUpButton) -> Apps {
    match popup_title(popup).as_str() {
        "active" => Apps::Active,
        "inactive" => Apps::Inactive,
        _ => Apps::All,
    }
}

fn order_title(o: Order) -> &'static str {
    match o {
        Order::Recent => "recent",
        Order::Created => "created",
        Order::Alphabetical => "alphabetical",
        Order::Space => "space",
    }
}

fn popup_order(popup: &NSPopUpButton) -> Order {
    match popup_title(popup).as_str() {
        "created" => Order::Created,
        "alphabetical" => Order::Alphabetical,
        "space" => Order::Space,
        _ => Order::Recent,
    }
}

fn style_title(s: Style) -> &'static str {
    match s {
        Style::Gallery => "gallery",
        Style::Icons => "icons",
        Style::List => "list",
    }
}

fn popup_style(popup: &NSPopUpButton) -> Style {
    match popup_title(popup).as_str() {
        "icons" => Style::Icons,
        "list" => Style::List,
        _ => Style::Gallery,
    }
}

fn size_title(s: Size) -> &'static str {
    match s {
        Size::Small => "small",
        Size::Medium => "medium",
        Size::Large => "large",
        Size::Auto => "auto",
    }
}

fn popup_size(popup: &NSPopUpButton) -> Size {
    match popup_title(popup).as_str() {
        "small" => Size::Small,
        "medium" => Size::Medium,
        "large" => Size::Large,
        _ => Size::Auto,
    }
}

fn on_release_title(o: OnRelease) -> &'static str {
    match o {
        OnRelease::Jump => "jump",
        OnRelease::Linger => "linger",
        OnRelease::Filter => "filter",
    }
}

fn popup_on_release(popup: &NSPopUpButton) -> OnRelease {
    match popup_title(popup).as_str() {
        "linger" => OnRelease::Linger,
        "filter" => OnRelease::Filter,
        _ => OnRelease::Jump,
    }
}

fn disposition_title(d: Disposition) -> &'static str {
    match d {
        Disposition::Show => "show",
        Disposition::End => "end",
        Disposition::Hide => "hide",
    }
}

fn popup_disposition(popup: &NSPopUpButton) -> Disposition {
    match popup_title(popup).as_str() {
        "end" => Disposition::End,
        "hide" => Disposition::Hide,
        _ => Disposition::Show,
    }
}
