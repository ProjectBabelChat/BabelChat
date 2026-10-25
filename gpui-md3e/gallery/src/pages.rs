//! The gallery pages, one function per navigation destination.

use gpui::{
    AnyElement, App, Context, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_md3e::{
    prelude::*,
    shapes::MaterialShape,
    theme::{SchemeVariant, TypeRole},
};

use crate::{Gallery, page, row, section};

pub fn buttons(gallery: &mut Gallery, cx: &mut Context<Gallery>) -> AnyElement {
    let mut grid = div().flex().flex_col().gap_6();
    for style in ButtonStyle::ALL {
        let mut line = row();
        for size in ButtonSize::ALL {
            line = line.child(
                Button::new(
                    SharedString::from(format!("{}-{}", style.label(), size.label())),
                    style.label(),
                )
                .style(style)
                .size(size)
                .icon(Icon::Favorite),
            );
        }
        grid = grid.child(line);
    }

    let mut toggles = row();
    for (ix, style) in [
        ButtonStyle::Elevated,
        ButtonStyle::Filled,
        ButtonStyle::Tonal,
        ButtonStyle::Outlined,
    ]
    .into_iter()
    .enumerate()
    {
        for (shape_ix, shape) in [ButtonShape::Round, ButtonShape::Square]
            .into_iter()
            .enumerate()
        {
            let key = ix * 2 + shape_ix;
            toggles = toggles.child(
                Button::new(("toggle", key), style.label())
                    .style(style)
                    .shape(shape)
                    .size(ButtonSize::Medium)
                    .icon(Icon::Star)
                    .selected(gallery.toggles[key])
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggles[key] = !this.toggles[key];
                        cx.notify();
                    })),
            );
        }
    }

    let mut square = row();
    for size in ButtonSize::ALL {
        square = square.child(
            Button::new(("square", size as usize), "Square")
                .shape(ButtonShape::Square)
                .style(ButtonStyle::Tonal)
                .size(size),
        );
    }

    let mut disabled = row();
    for style in ButtonStyle::ALL {
        disabled = disabled.child(
            Button::new(("disabled", style as usize), style.label())
                .style(style)
                .icon(Icon::Lock)
                .disabled(true),
        );
    }

    page("Buttons", cx)
        .child(
            section(
                "Styles and sizes",
                "Five emphasis levels in the five M3 Expressive sizes. Press a button to see the corners morph on a fast spatial spring.",
                cx,
            )
            .child(grid),
        )
        .child(
            section(
                "Toggle buttons",
                "Round buttons turn square when selected and square buttons turn round. Colors cross-fade on an effects spring.",
                cx,
            )
            .child(toggles),
        )
        .child(section("Square shape", "The square variant at every size.", cx).child(square))
        .child(section("Disabled", "", cx).child(disabled))
        .into_any_element()
}

pub fn icon_buttons(gallery: &mut Gallery, cx: &mut Context<Gallery>) -> AnyElement {
    let mut grid = div().flex().flex_col().gap_6();
    for (style_ix, style) in IconButtonStyle::ALL.into_iter().enumerate() {
        let mut line = row();
        for size in ButtonSize::ALL {
            line = line.child(
                IconButton::new(
                    SharedString::from(format!("icon-{}-{}", style.label(), size.label())),
                    Icon::Settings,
                )
                .style(style)
                .size(size),
            );
        }
        let key = style_ix;
        line = line.child(
            IconButton::new(("icon-toggle", key), Icon::Favorite)
                .style(style)
                .size(ButtonSize::Medium)
                .selected(gallery.icon_toggles[key])
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.icon_toggles[key] = !this.icon_toggles[key];
                    cx.notify();
                })),
        );
        grid = grid.child(line);
    }

    let mut widths = row();
    for (ix, width) in [
        IconButtonWidth::Narrow,
        IconButtonWidth::Default,
        IconButtonWidth::Wide,
    ]
    .into_iter()
    .enumerate()
    {
        widths = widths.child(
            IconButton::new(("width", ix), Icon::Bookmark)
                .style(IconButtonStyle::Tonal)
                .size(ButtonSize::Medium)
                .width(width),
        );
    }

    let mut fabs = row();
    for (ix, size) in FabSize::ALL.into_iter().enumerate() {
        fabs = fabs.child(Fab::new(("fab", ix), Icon::Add).size(size));
    }
    let mut fab_colors = row();
    for (ix, color) in FabColor::ALL.into_iter().enumerate() {
        fab_colors = fab_colors.child(Fab::new(("fab-color", ix), Icon::Edit).color(color));
    }

    let expanded = gallery.fab_expanded;
    let mut extended = row();
    for (ix, size) in FabSize::ALL.into_iter().enumerate() {
        extended = extended.child(
            Fab::new(("extended", ix), Icon::Edit)
                .label("Compose")
                .size(size)
                .expanded(expanded)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.fab_expanded = !this.fab_expanded;
                    cx.notify();
                })),
        );
    }

    page("Icon buttons & FAB", cx)
        .child(
            section(
                "Icon buttons",
                "Standard, filled, tonal and outlined in five sizes. The last button in each row is a toggle that morphs from round to square.",
                cx,
            )
            .child(grid),
        )
        .child(section("Widths", "Narrow, default and wide.", cx).child(widths))
        .child(section("FAB sizes", "FAB, medium FAB and large FAB.", cx).child(fabs))
        .child(section("FAB colors", "Container and solid color sets.", cx).child(fab_colors))
        .child(
            section(
                "Extended FAB",
                "Click to collapse or expand: the width follows a spatial spring and the label fades.",
                cx,
            )
            .child(extended),
        )
        .into_any_element()
}

pub fn groups(gallery: &mut Gallery, cx: &mut Context<Gallery>) -> AnyElement {
    let formatting = [
        (Icon::FormatBold, "Bold"),
        (Icon::FormatItalic, "Italic"),
        (Icon::FormatUnderlined, "Underline"),
    ];
    let mut format_group = ButtonGroup::new("format");
    for (ix, (glyph, label)) in formatting.into_iter().enumerate() {
        format_group = format_group.child(
            IconButton::new(("format", ix), glyph)
                .label(label)
                .style(IconButtonStyle::Tonal)
                .size(ButtonSize::Medium)
                .width(IconButtonWidth::Wide)
                .selected(gallery.formatting[ix])
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.formatting[ix] = !this.formatting[ix];
                    cx.notify();
                })),
        );
    }

    let actions = ButtonGroup::new("actions")
        .child(
            Button::new("reply", "Reply")
                .style(ButtonStyle::Filled)
                .size(ButtonSize::Medium)
                .icon(Icon::Send),
        )
        .child(
            Button::new("translate", "Translate")
                .style(ButtonStyle::Tonal)
                .size(ButtonSize::Medium)
                .icon(Icon::Translate),
        )
        .child(
            IconButton::new("more", Icon::MoreHoriz)
                .style(IconButtonStyle::Tonal)
                .size(ButtonSize::Medium),
        );

    let periods = ["Day", "Week", "Month", "Year"];
    let mut single = ButtonGroup::new("single").connected(true);
    for (ix, label) in periods.into_iter().enumerate() {
        single = single.child(
            Button::new(("period", ix), label)
                .style(ButtonStyle::Tonal)
                .size(ButtonSize::Small)
                .selected(gallery.single_select == ix)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.single_select = ix;
                    cx.notify();
                })),
        );
    }

    let filters = [
        (Icon::Chat, "Chats"),
        (Icon::Group, "Groups"),
        (Icon::Translate, "Translated"),
        (Icon::Star, "Starred"),
    ];
    let mut multi = ButtonGroup::new("multi").connected(true).fill(true);
    for (ix, (glyph, label)) in filters.into_iter().enumerate() {
        multi = multi.child(
            Button::new(("filter", ix), label)
                .style(ButtonStyle::Filled)
                .size(ButtonSize::Medium)
                .icon(glyph)
                .selected(gallery.multi_select[ix])
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.multi_select[ix] = !this.multi_select[ix];
                    cx.notify();
                })),
        );
    }

    page("Button groups", cx)
        .child(
            section(
                "Standard groups",
                "Press and hold: the pressed button widens and squeezes its neighbors.",
                cx,
            )
            .child(row().child(format_group))
            .child(row().child(actions)),
        )
        .child(
            section(
                "Connected, single select",
                "Inner corners tighten on press and round out fully on selection.",
                cx,
            )
            .child(row().child(single)),
        )
        .child(
            section(
                "Connected, multi select",
                "Buttons share the full width.",
                cx,
            )
            .child(div().w(px(720.0)).child(multi)),
        )
        .into_any_element()
}

pub fn progress(gallery: &mut Gallery, cx: &mut Context<Gallery>) -> AnyElement {
    let value = gallery.progress;
    let mut steps = ButtonGroup::new("progress-steps").connected(true);
    for (ix, target) in [0.0f32, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
        steps = steps.child(
            Button::new(("step", ix), format!("{}%", (target * 100.0) as u32))
                .style(ButtonStyle::Tonal)
                .selected((value - target).abs() < 0.01)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.progress = target;
                    cx.notify();
                })),
        );
    }

    page("Loading & progress", cx)
        .child(
            section(
                "Loading indicator",
                "Seven shapes morph on a bouncy spring while the indicator spins.",
                cx,
            )
            .child(
                row()
                    .gap_8()
                    .child(LoadingIndicator::new("loading"))
                    .child(LoadingIndicator::new("loading-contained").contained(true))
                    .child(LoadingIndicator::new("loading-large").size(px(96.0)))
                    .child(
                        LoadingIndicator::new("loading-progress")
                            .contained(true)
                            .progress(value)
                            .size(px(72.0)),
                    ),
            ),
        )
        .child(
            section(
                "Determinate",
                "Pick a value: progress glides on a spring, and the wave flattens near the ends.",
                cx,
            )
            .child(row().child(steps))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .w(px(560.0))
                    .child(LinearProgress::new("linear").progress(value))
                    .child(
                        LinearProgress::new("linear-wavy")
                            .progress(value)
                            .style(ProgressStyle::Wavy),
                    ),
            )
            .child(
                row()
                    .gap_8()
                    .child(CircularProgress::new("circular").progress(value))
                    .child(
                        CircularProgress::new("circular-wavy")
                            .progress(value)
                            .style(ProgressStyle::Wavy),
                    ),
            ),
        )
        .child(
            section("Indeterminate", "", cx)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_6()
                        .w(px(560.0))
                        .child(LinearProgress::new("linear-indeterminate"))
                        .child(
                            LinearProgress::new("linear-indeterminate-wavy")
                                .style(ProgressStyle::Wavy),
                        ),
                )
                .child(
                    row()
                        .gap_8()
                        .child(CircularProgress::new("circular-indeterminate"))
                        .child(
                            CircularProgress::new("circular-indeterminate-wavy")
                                .style(ProgressStyle::Wavy),
                        ),
                ),
        )
        .into_any_element()
}

pub fn shapes(_window: &mut Window, cx: &mut Context<Gallery>) -> AnyElement {
    let colors = cx.md().colors;
    let mut grid = div().flex().flex_row().flex_wrap().gap_6();
    for (ix, shape) in MaterialShape::ALL.iter().copied().enumerate() {
        grid = grid.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .w(px(104.0))
                .child(
                    ShapeView::new(("shape", ix), shape)
                        .size(px(80.0))
                        .color(colors.primary)
                        .hover_color(colors.tertiary)
                        .morph_on_hover(MaterialShape::Circle),
                )
                .child(
                    div()
                        .md_type(TypeRole::LabelMedium, cx)
                        .text_color(colors.on_surface_variant)
                        .child(shape.label()),
                ),
        );
    }
    page("Shapes", cx)
        .child(
            section(
                "Shape library",
                "The 35 M3 Expressive shapes. Hover one to morph it into a circle.",
                cx,
            )
            .child(grid),
        )
        .into_any_element()
}

const SEEDS: [(u32, &str); 8] = [
    (0x6750A4, "Baseline"),
    (0x0061A4, "Blue"),
    (0x006A60, "Teal"),
    (0x386A20, "Green"),
    (0x8B5000, "Amber"),
    (0xB3261E, "Red"),
    (0x984061, "Pink"),
    (0x5C5F61, "Gray"),
];

fn update_theme(cx: &mut App, f: impl FnOnce(&mut SchemeConfig)) {
    let theme = cx.global_mut::<Theme>();
    let mut config = theme.config();
    f(&mut config);
    theme.set_config(config);
    cx.refresh_windows();
}

pub fn theme(window: &mut Window, cx: &mut Context<Gallery>) -> AnyElement {
    let colors = cx.md().colors;
    let config = cx.md().config();

    let mut seeds = row();
    for (ix, (seed, label)) in SEEDS.into_iter().enumerate() {
        let selected = config.seed == seed;
        seeds = seeds.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(seed_swatch(ix, seed, selected, window, cx))
                .child(
                    div()
                        .md_type(TypeRole::LabelSmall, cx)
                        .text_color(colors.on_surface_variant)
                        .child(label),
                ),
        );
    }

    let mut variants = row();
    for (ix, variant) in SchemeVariant::ALL.into_iter().enumerate() {
        variants = variants.child(
            Button::new(("variant", ix), variant.label())
                .style(ButtonStyle::Outlined)
                .size(ButtonSize::XSmall)
                .selected(config.variant == variant)
                .on_click(move |_, _, cx| update_theme(cx, |c| c.variant = variant)),
        );
    }

    let mut mode = ButtonGroup::new("mode").connected(true);
    for (ix, (dark, label, glyph)) in [
        (false, "Light", Icon::LightMode),
        (true, "Dark", Icon::DarkMode),
    ]
    .into_iter()
    .enumerate()
    {
        mode = mode.child(
            Button::new(("mode", ix), label)
                .style(ButtonStyle::Tonal)
                .icon(glyph)
                .selected(config.dark == dark)
                .on_click(move |_, _, cx| update_theme(cx, |c| c.dark = dark)),
        );
    }
    let mut contrast = ButtonGroup::new("contrast").connected(true);
    for (ix, (level, label)) in [(0.0, "Standard"), (0.5, "Medium"), (1.0, "High")]
        .into_iter()
        .enumerate()
    {
        contrast = contrast.child(
            Button::new(("contrast", ix), label)
                .style(ButtonStyle::Tonal)
                .selected((config.contrast - level).abs() < 0.01)
                .on_click(move |_, _, cx| update_theme(cx, |c| c.contrast = level)),
        );
    }

    let roles: [(&str, Hsla, Hsla); 12] = [
        ("Primary", colors.primary, colors.on_primary),
        (
            "Primary container",
            colors.primary_container,
            colors.on_primary_container,
        ),
        ("Secondary", colors.secondary, colors.on_secondary),
        (
            "Secondary container",
            colors.secondary_container,
            colors.on_secondary_container,
        ),
        ("Tertiary", colors.tertiary, colors.on_tertiary),
        (
            "Tertiary container",
            colors.tertiary_container,
            colors.on_tertiary_container,
        ),
        ("Error", colors.error, colors.on_error),
        (
            "Error container",
            colors.error_container,
            colors.on_error_container,
        ),
        ("Surface", colors.surface, colors.on_surface),
        (
            "Surface container",
            colors.surface_container,
            colors.on_surface,
        ),
        (
            "Surface container highest",
            colors.surface_container_highest,
            colors.on_surface,
        ),
        (
            "Inverse surface",
            colors.inverse_surface,
            colors.inverse_on_surface,
        ),
    ];
    let mut swatches = div().flex().flex_row().flex_wrap().gap_3();
    for (name, bg, fg) in roles {
        swatches = swatches.child(
            div()
                .w(px(180.0))
                .h(px(88.0))
                .p_3()
                .rounded(px(16.0))
                .bg(bg)
                .text_color(fg)
                .border_1()
                .border_color(colors.outline_variant)
                .md_type(TypeRole::LabelLarge, cx)
                .child(name),
        );
    }

    let typography = cx.md().typography.clone();
    let mut type_scale = div().flex().flex_col().gap_3();
    for role in TypeRole::ALL {
        let style = typography.style(role);
        type_scale = type_scale.child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_baseline()
                .gap_6()
                .child(
                    div()
                        .w(px(160.0))
                        .flex_none()
                        .md_type(TypeRole::LabelMedium, cx)
                        .text_color(colors.on_surface_variant)
                        .child(format!(
                            "{} · {}/{}",
                            role.label(),
                            style.size.as_f32(),
                            style.line_height.as_f32()
                        )),
                )
                .child(div().md_type(role, cx).child("言語の壁を壊す Babel"))
                .child(
                    div()
                        .md_type_emphasized(role, cx)
                        .text_color(colors.primary)
                        .child("Emphasized"),
                ),
        );
    }

    page("Theme", cx)
        .child(
            section(
                "Seed color",
                "Every color role is generated from one seed with the 2025 dynamic color spec. Changes cross-fade.",
                cx,
            )
            .child(seeds),
        )
        .child(section("Scheme variant", "", cx).child(variants))
        .child(
            section("Mode and contrast", "", cx)
                .child(row().child(mode).child(contrast)),
        )
        .child(section("Color roles", "", cx).child(swatches))
        .child(
            section(
                "Type scale",
                "Baseline and emphasized styles, with CJK fallback.",
                cx,
            )
            .child(type_scale),
        )
        .into_any_element()
}

pub fn selection(gallery: &mut Gallery, cx: &mut Context<Gallery>) -> AnyElement {
    let labelled = |control: AnyElement, label: &'static str, cx: &App| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(control)
            .child(div().md_type(TypeRole::BodyLarge, cx).child(label))
            .into_any_element()
    };

    let mut switches = row().gap_8();
    for (ix, label) in ["Auto-translate", "Show original", "Notifications"]
        .into_iter()
        .enumerate()
    {
        let switch = Switch::new(("switch", ix), gallery.switches[ix])
            .icons(ix == 2)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.switches[ix] = !this.switches[ix];
                cx.notify();
            }));
        switches = switches.child(labelled(switch.into_any_element(), label, cx));
    }
    switches = switches.child(labelled(
        Switch::new("switch-disabled", true)
            .disabled(true)
            .into_any_element(),
        "Disabled",
        cx,
    ));

    let mut checks = row().gap_8();
    for (ix, label) in ["Japanese", "English", "All languages"]
        .into_iter()
        .enumerate()
    {
        let checkbox = Checkbox::new(("check", ix), gallery.checks[ix]).on_click(cx.listener(
            move |this, _, _, cx| {
                this.checks[ix] = match this.checks[ix] {
                    CheckState::Checked => CheckState::Unchecked,
                    _ => CheckState::Checked,
                };
                cx.notify();
            },
        ));
        checks = checks.child(labelled(checkbox.into_any_element(), label, cx));
    }
    checks = checks.child(labelled(
        Checkbox::new("check-error", CheckState::Unchecked)
            .error(true)
            .into_any_element(),
        "Error",
        cx,
    ));

    let mut radios = row().gap_8();
    for (ix, label) in ["Local model", "Cloud LLM", "Self-hosted"]
        .into_iter()
        .enumerate()
    {
        let radio = RadioButton::new(("radio", ix), gallery.radio == ix).on_click(cx.listener(
            move |this, _, _, cx| {
                this.radio = ix;
                cx.notify();
            },
        ));
        radios = radios.child(labelled(radio.into_any_element(), label, cx));
    }

    let sliders = div()
        .flex()
        .flex_col()
        .gap_8()
        .w(px(480.0))
        .pt_8()
        .child(Slider::new("volume", gallery.volume).on_change(cx.listener(
            |this, value: &f32, _, cx| {
                this.volume = *value;
                cx.notify();
            },
        )))
        .child(
            Slider::new("stepped", gallery.stepped)
                .steps(5)
                .on_change(cx.listener(|this, value: &f32, _, cx| {
                    this.stepped = *value;
                    cx.notify();
                })),
        );

    let mut chips = row();
    for (ix, label) in ["Unread", "Mentions", "Translated", "Pinned"]
        .into_iter()
        .enumerate()
    {
        chips = chips.child(
            Chip::new(("filter-chip", ix), label)
                .kind(ChipKind::Filter)
                .selected(gallery.filters[ix])
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filters[ix] = !this.filters[ix];
                    cx.notify();
                })),
        );
    }
    let other_chips = row()
        .child(Chip::new("assist", "Translate thread").icon(Icon::Translate))
        .child(
            Chip::new("assist-elevated", "Add to calendar")
                .icon(Icon::Schedule)
                .elevated(true),
        )
        .child(
            Chip::new("input", "日本語")
                .kind(ChipKind::Input)
                .icon(Icon::Language),
        )
        .child(Chip::new("suggestion", "Sounds good!").kind(ChipKind::Suggestion));

    let fields = div()
        .flex()
        .flex_col()
        .gap_6()
        .w(px(420.0))
        .child(gallery.name_field.clone())
        .child(gallery.search_field.clone());

    page("Selection", cx)
        .child(
            section(
                "Text fields",
                "Labels float on a spring; the indicator thickens on focus. Japanese input goes through the platform IME.",
                cx,
            )
            .child(fields),
        )
        .child(
            section(
                "Switches",
                "The handle grows when selected and swells while pressed.",
                cx,
            )
            .child(switches),
        )
        .child(section("Checkboxes", "The check mark draws itself in.", cx).child(checks))
        .child(section("Radio buttons", "", cx).child(radios))
        .child(
            section(
                "Sliders",
                "Drag the handle: it narrows and a value bubble springs up.",
                cx,
            )
            .child(sliders),
        )
        .child(
            section("Chips", "Filter chips widen as their check mark slides in.", cx)
                .child(chips)
                .child(other_chips),
        )
        .into_any_element()
}

pub fn surfaces(gallery: &mut Gallery, cx: &mut Context<Gallery>) -> AnyElement {
    let colors = cx.md().colors;

    let mut cards = row().items_start();
    for (ix, (style, title)) in [
        (CardStyle::Elevated, "Elevated"),
        (CardStyle::Filled, "Filled"),
        (CardStyle::Outlined, "Outlined"),
    ]
    .into_iter()
    .enumerate()
    {
        cards = cards.child(
            Card::new(("card", ix))
                .style(style)
                .on_click(|_, _, _| {})
                .child(
                    div()
                        .w(px(220.0))
                        .h(px(96.0))
                        .rounded(px(8.0))
                        .bg(colors.tertiary_container),
                )
                .child(div().md_type(TypeRole::TitleMedium, cx).child(title))
                .child(
                    div()
                        .md_type(TypeRole::BodyMedium, cx)
                        .text_color(colors.on_surface_variant)
                        .child("Hover to lift, press to ripple."),
                ),
        );
    }

    let chats = [
        ("Aiko", "明日の打ち合わせ、15時からで大丈夫？", Icon::Person),
        (
            "Ben",
            "Translated from English · See you tomorrow!",
            Icon::Person,
        ),
        ("BabelChat team", "Welcome to the beta 🎉", Icon::Forum),
        ("Carla", "¿Probamos la traducción automática?", Icon::Person),
    ];
    let mut list = div().flex().flex_col().gap(px(2.0)).w(px(480.0));
    let count = chats.len();
    for (ix, (name, message, glyph)) in chats.into_iter().enumerate() {
        list = list.child(
            ListItem::new(("chat", ix), name)
                .supporting(message)
                .leading(
                    div()
                        .size(px(40.0))
                        .rounded_full()
                        .bg(colors.primary_container)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph).color(colors.on_primary_container)),
                )
                .trailing(div().md_type(TypeRole::LabelSmall, cx).child(format!(
                    "{}:0{}",
                    9 + ix,
                    ix
                )))
                .segmented(ix == 0, ix + 1 == count)
                .selected(gallery.selected_chat == ix)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_chat = ix;
                    cx.notify();
                })),
        );
    }

    let toolbar = Toolbar::new()
        .child(IconButton::new("tb-bold", Icon::FormatBold))
        .child(IconButton::new("tb-italic", Icon::FormatItalic))
        .child(IconButton::new("tb-attach", Icon::AttachFile))
        .child(IconButton::new("tb-image", Icon::Image))
        .child(
            IconButton::new("tb-send", Icon::Send)
                .style(IconButtonStyle::Filled)
                .width(IconButtonWidth::Wide),
        );
    let vibrant = Toolbar::new()
        .vibrant(true)
        .child(IconButton::new("vb-mic", Icon::Mic))
        .child(IconButton::new("vb-translate", Icon::Translate))
        .child(IconButton::new("vb-emoji", Icon::AddReaction));

    let mut splits = row();
    for (ix, size) in [ButtonSize::Small, ButtonSize::Medium, ButtonSize::Large]
        .into_iter()
        .enumerate()
    {
        splits = splits.child(
            SplitButton::new(("split", ix), "Send")
                .icon(Icon::Send)
                .size(size)
                .style(if ix == 1 {
                    ButtonStyle::Tonal
                } else {
                    ButtonStyle::Filled
                })
                .open(gallery.split_open)
                .on_toggle(cx.listener(|this, _, _, cx| {
                    this.split_open = !this.split_open;
                    cx.notify();
                })),
        );
    }

    let fab_menu = FabMenu::new("fab-menu", Icon::Add)
        .open(gallery.fab_menu_open)
        .on_toggle(cx.listener(|this, _, _, cx| {
            this.fab_menu_open = !this.fab_menu_open;
            cx.notify();
        }))
        .item(FabMenuItem::new(Icon::Chat, "New chat"))
        .item(FabMenuItem::new(Icon::Group, "New group"))
        .item(FabMenuItem::new(Icon::Translate, "Translate a file"))
        .item(FabMenuItem::new(Icon::PhotoCamera, "Share a photo"));

    let snackbar_visible = gallery.snackbar;
    let show_snackbar = Button::new("show-snackbar", "Show snackbar")
        .style(ButtonStyle::Tonal)
        .on_click(cx.listener(|this, _, _, cx| {
            this.snackbar = true;
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(4))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.snackbar = false;
                    cx.notify();
                });
            })
            .detach();
        }));

    page("Surfaces", cx)
        .child(section("Cards", "Elevated, filled and outlined.", cx).child(cards))
        .child(
            section(
                "Segmented list",
                "M3 Expressive grouped list items; the selected item rounds out.",
                cx,
            )
            .child(list),
        )
        .child(
            section("Floating toolbars", "Standard and vibrant.", cx)
                .child(row().child(toolbar).child(vibrant)),
        )
        .child(
            section(
                "Split buttons",
                "Hover the inner edge; open the menu to see the trailing button turn round and its arrow flip.",
                cx,
            )
            .child(splits),
        )
        .child(
            section(
                "FAB menu and snackbar",
                "The FAB morphs into a close button while the actions stagger in.",
                cx,
            )
            .child(
                row()
                    .items_end()
                    .justify_between()
                    .w(px(760.0))
                    .pt(px(300.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_4()
                            .w(px(420.0))
                            .child(show_snackbar)
                            .child(
                                div().h(px(56.0)).child(
                                    Snackbar::new("snackbar", "Message translated to 日本語")
                                        .action("Undo", cx.listener(|this, _, _, cx| {
                                            this.snackbar = false;
                                            cx.notify();
                                        }))
                                        .visible(snackbar_visible),
                                ),
                            ),
                    )
                    .child(fab_menu),
            ),
        )
        .into_any_element()
}

/// A seed color that turns from a circle into a rounded square when chosen.
fn seed_swatch(
    ix: usize,
    seed: u32,
    selected: bool,
    window: &mut Window,
    cx: &mut Context<Gallery>,
) -> AnyElement {
    let radius = animate(
        ("seed-radius", ix),
        if selected { 16.0 } else { 28.0 },
        MotionToken::FastSpatial,
        window,
        cx,
    );
    let check = animate(
        ("seed-check", ix),
        if selected { 1.0 } else { 0.0 },
        MotionToken::FastSpatial,
        window,
        cx,
    );
    div()
        .id(("seed", ix))
        .size(px(56.0))
        .rounded(px(radius))
        .bg(gpui::rgb(seed))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .child(
            icon(Icon::Check)
                .size(px(28.0 * check.max(0.0)))
                .color(gpui::white()),
        )
        .on_click(move |_, _, cx| update_theme(cx, |c| c.seed = seed))
        .into_any_element()
}
