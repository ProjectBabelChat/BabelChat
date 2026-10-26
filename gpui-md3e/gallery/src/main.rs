use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, KeyBinding, SharedString, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, size,
};
use gpui_md3e::{Md3eAssets, prelude::*, theme::SchemeConfig};
use gpui_platform::application;

mod chat;
mod pages;

actions!(gallery, [FocusNext, FocusPrev, Quit]);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Chat,
    Buttons,
    IconButtons,
    Groups,
    Progress,
    Selection,
    Surfaces,
    Shapes,
    Theme,
}

impl Page {
    const ALL: [Page; 9] = [
        Page::Chat,
        Page::Buttons,
        Page::IconButtons,
        Page::Groups,
        Page::Progress,
        Page::Selection,
        Page::Surfaces,
        Page::Shapes,
        Page::Theme,
    ];

    fn item(self) -> RailItem {
        match self {
            Page::Chat => RailItem::new(Icon::Forum, "Chat"),
            Page::Buttons => RailItem::new(Icon::Label, "Buttons"),
            Page::IconButtons => RailItem::new(Icon::Favorite, "Icons & FAB"),
            Page::Groups => RailItem::new(Icon::Tune, "Groups"),
            Page::Progress => RailItem::new(Icon::Refresh, "Progress"),
            Page::Selection => RailItem::new(Icon::CheckCircle, "Selection"),
            Page::Surfaces => RailItem::new(Icon::Inbox, "Surfaces"),
            Page::Shapes => RailItem::new(Icon::AutoAwesome, "Shapes"),
            Page::Theme => RailItem::new(Icon::Palette, "Theme"),
        }
    }
}

/// Demo state shared by the gallery pages.
pub struct Gallery {
    focus_handle: FocusHandle,
    page: Page,
    rail_expanded: bool,
    pub toggles: [bool; 8],
    pub icon_toggles: [bool; 8],
    pub fab_expanded: bool,
    pub progress: f32,
    pub single_select: usize,
    pub multi_select: [bool; 4],
    pub formatting: [bool; 3],
    pub switches: [bool; 3],
    pub checks: [CheckState; 3],
    pub radio: usize,
    pub volume: f32,
    pub stepped: f32,
    pub filters: [bool; 4],
    pub split_open: bool,
    pub fab_menu_open: bool,
    pub snackbar: bool,
    pub selected_chat: usize,
    pub messages: Vec<chat::Message>,
    pub composer: gpui::Entity<TextField>,
    pub name_field: gpui::Entity<TextField>,
    pub search_field: gpui::Entity<TextField>,
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Theme::tick(window, cx);
        let colors = cx.md().colors;
        let page = self.page;

        let mut rail = NavigationRail::new("rail")
            .expanded(self.rail_expanded)
            .selected(Page::ALL.iter().position(|p| *p == page).unwrap_or(0))
            .header(
                Fab::new("rail-fab", Icon::Edit)
                    .label("Compose")
                    .expanded(self.rail_expanded)
                    .color(FabColor::TertiaryContainer)
                    .lowered(true),
            )
            .on_toggle(cx.listener(|this, _, _, cx| {
                this.rail_expanded = !this.rail_expanded;
                cx.notify();
            }))
            .on_select(cx.listener(|this, ix: &usize, _, cx| {
                this.page = Page::ALL[*ix];
                cx.notify();
            }));
        for page in Page::ALL {
            rail = rail.item(page.item());
        }

        let content: AnyElement = match page {
            Page::Chat => chat::chat(self, window, cx),
            Page::Buttons => pages::buttons(self, cx),
            Page::IconButtons => pages::icon_buttons(self, cx),
            Page::Groups => pages::groups(self, cx),
            Page::Progress => pages::progress(self, cx),
            Page::Selection => pages::selection(self, cx),
            Page::Surfaces => pages::surfaces(self, cx),
            Page::Shapes => pages::shapes(window, cx),
            Page::Theme => pages::theme(window, cx),
        };

        div()
            .id("gallery")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &FocusNext, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &FocusPrev, window, cx| window.focus_prev(cx)))
            .size_full()
            .flex()
            .flex_row()
            .bg(colors.surface_container)
            .text_color(colors.on_surface)
            .md_type(TypeRole::BodyMedium, cx)
            .child(rail)
            .child(
                div().flex_1().h_full().py_2().pr_2().child(
                    div()
                        .id("page")
                        .size_full()
                        .overflow_y_scroll()
                        .rounded(px(28.0))
                        .bg(colors.surface)
                        .child(content),
                ),
            )
    }
}

fn initial_page() -> Page {
    std::env::var("MD3E_PAGE")
        .ok()
        .and_then(|page| page.parse::<usize>().ok())
        .and_then(|ix| Page::ALL.get(ix).copied())
        .unwrap_or(Page::Chat)
}

fn main() {
    application().with_assets(Md3eAssets).run(|cx: &mut App| {
        gpui_md3e::init(SchemeConfig::default(), cx);
        cx.bind_keys([
            KeyBinding::new("tab", FocusNext, None),
            KeyBinding::new("shift-tab", FocusPrev, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(1280.0), px(900.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    app_id: Some("md3e-gallery".into()),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        let focus_handle = cx.focus_handle();
                        window.focus(&focus_handle, cx);
                        let gallery = cx.entity().downgrade();
                        let composer = cx.new(|cx| {
                            TextField::new(cx)
                                .label("Message #babel-dev")
                                .placeholder("Write in any language")
                                .on_submit(move |text, _, cx| {
                                    gallery
                                        .update(cx, |gallery: &mut Gallery, cx| {
                                            gallery.send(text, cx)
                                        })
                                        .unwrap_or(false)
                                })
                        });
                        let name_field = cx.new(|cx| {
                            TextField::new(cx)
                                .label("Display name")
                                .supporting_text("Shown to people in every language")
                                .leading_icon(Icon::Person)
                        });
                        let search_field = cx.new(|cx| {
                            TextField::new(cx)
                                .style(TextFieldStyle::Outlined)
                                .label("Search messages")
                                .placeholder("翻訳済みのメッセージも検索")
                                .trailing_icon(Icon::Search)
                        });
                        Gallery {
                            focus_handle,
                            page: initial_page(),
                            rail_expanded: false,
                            toggles: [false; 8],
                            icon_toggles: [false; 8],
                            fab_expanded: true,
                            progress: 0.35,
                            single_select: 1,
                            multi_select: [true, false, true, false],
                            formatting: [true, false, false],
                            switches: [true, false, true],
                            checks: [
                                CheckState::Checked,
                                CheckState::Unchecked,
                                CheckState::Indeterminate,
                            ],
                            radio: 0,
                            volume: 0.6,
                            stepped: 0.4,
                            filters: [true, false, false, true],
                            split_open: false,
                            fab_menu_open: false,
                            snackbar: false,
                            selected_chat: 0,
                            messages: chat::seed_messages(),
                            composer,
                            name_field,
                            search_field,
                        }
                    })
                },
            )
            .unwrap();
        gpui_md3e::script::run_from_env("MD3E_SCRIPT", window.into(), cx);
        cx.activate(true);
    });
}

/// A titled block of related samples.
pub fn section(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    cx: &App,
) -> gpui::Div {
    let colors = cx.md().colors;
    div().flex().flex_col().gap_4().child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().md_type(TypeRole::TitleLarge, cx).child(title.into()))
            .child(
                div()
                    .md_type(TypeRole::BodyMedium, cx)
                    .text_color(colors.on_surface_variant)
                    .child(description.into()),
            ),
    )
}

/// The scrolling column every page renders into.
pub fn page(title: &'static str, cx: &App) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_10()
        .p_10()
        .child(div().md_type(TypeRole::DisplaySmall, cx).child(title))
}

pub fn row() -> gpui::Div {
    div().flex().flex_row().flex_wrap().items_center().gap_4()
}
