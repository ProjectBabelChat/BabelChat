//! The guild rail, the channel list with the user panel, the guild menu and
//! the member list.

use babel_sdk::{Channel, ChannelKind, GuildView};
use gpui::prelude::*;
use gpui::{AnyElement, Context, IntoElement, SharedString, div, px};
use gpui_md3e::{motion::animate, prelude::*};

use super::{ChatView, Dialog};
use crate::{i18n::language_name, widgets};

const RAIL_WIDTH: f32 = 72.0;
const SIDEBAR_WIDTH: f32 = 248.0;
const MEMBERS_WIDTH: f32 = 220.0;

impl ChatView {
    pub(super) fn render_guild_rail(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let dark = cx.md().is_dark();
        let mut rail = div()
            .id("guild-rail")
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .py_3();

        let guilds = self.session.guilds();
        let any = !guilds.is_empty();
        for view in guilds {
            let id = view.guild.id.clone();
            let selected = self.guild.as_deref() == Some(&id);
            let unread = self.session.guild_unread(&id);
            let element_id = SharedString::from(format!("guild-{id}"));
            // Discord's rounding: a circle that turns into a rounded square
            // when selected.
            let radius = animate(
                (element_id.clone(), 0usize),
                if selected { 16.0 } else { 24.0 },
                MotionToken::FastSpatial,
                window,
                cx,
            );
            let pill = animate(
                (element_id.clone(), 1usize),
                if selected {
                    40.0
                } else if unread.unread {
                    8.0
                } else {
                    0.0
                },
                MotionToken::FastSpatial,
                window,
                cx,
            );
            let (bg, fg) = if selected {
                (colors.primary, colors.on_primary)
            } else {
                (
                    widgets::name_color(&view.guild.name, dark),
                    colors.on_surface,
                )
            };
            let mut icon_box = div()
                .id(element_id)
                .relative()
                .size(px(48.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius))
                .bg(bg)
                .text_color(fg)
                .md_type(TypeRole::TitleMedium, cx)
                .cursor_pointer()
                .hover(|style| style.opacity(0.9))
                .child(widgets::initial(&view.guild.name))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.select_guild(&id, window, cx)),
                );
            if unread.mentions > 0 {
                icon_box = icon_box.child(
                    div()
                        .absolute()
                        .bottom(px(-4.0))
                        .right(px(-4.0))
                        .child(widgets::badge(unread.mentions, cx)),
                );
            }
            rail = rail.child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(24.0 - pill / 2.0))
                            .w(px(4.0))
                            .h(px(pill))
                            .rounded_r(px(4.0))
                            .bg(colors.on_surface),
                    )
                    .child(icon_box),
            );
        }

        let t = self.t();
        rail.when(any, |rail| {
            rail.child(
                div()
                    .w(px(32.0))
                    .h(px(2.0))
                    .rounded_full()
                    .bg(colors.outline_variant),
            )
        })
        .child(
            IconButton::new("add-guild", Icon::Add)
                .style(IconButtonStyle::Tonal)
                .size(ButtonSize::Medium)
                .label(t.add_guild)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.dialog = Some(Dialog::add_guild(this, window, cx));
                    cx.notify();
                })),
        )
        .into_any_element()
    }

    pub(super) fn render_sidebar(
        &mut self,
        guild: Option<&GuildView>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let mut sidebar = div()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex()
            .flex_col();

        match guild {
            Some(guild) => {
                let name = guild.guild.name.clone();
                sidebar = sidebar.child(
                    div()
                        .id("guild-header")
                        .flex_none()
                        .h(px(64.0))
                        .px_4()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .rounded(px(16.0))
                        .hover(|style| style.bg(colors.surface_container_high))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.guild_menu = !this.guild_menu;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .md_type(TypeRole::TitleMedium, cx)
                                .child(name),
                        )
                        .child(icon(Icon::ExpandMore).color(colors.on_surface_variant)),
                );
                sidebar = sidebar.child(self.render_channel_list(guild, cx));
            }
            None => {
                sidebar = sidebar.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_4()
                        .pt_6()
                        .child(
                            div()
                                .md_type(TypeRole::TitleMedium, cx)
                                .child(t.no_guilds_title),
                        )
                        .child(widgets::supporting(t.no_guilds_body, cx)),
                );
            }
        }
        sidebar.child(self.render_user_panel(cx)).into_any_element()
    }

    fn render_channel_list(&mut self, guild: &GuildView, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let mut list = div()
            .id("channel-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_0p5()
            .px_2();

        let mut header = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(36.0))
            .pl_3()
            .child(
                div()
                    .flex_1()
                    .md_type(TypeRole::LabelMedium, cx)
                    .text_color(colors.on_surface_variant)
                    .child(t.channels),
            );
        if self.is_owner(guild) {
            header = header.child(
                IconButton::new("create-channel", Icon::Add)
                    .size(ButtonSize::XSmall)
                    .label(t.create_channel)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dialog = Some(Dialog::create_channel(this, window, cx));
                        cx.notify();
                    })),
            );
        }
        list = list.child(header);

        let channels = ordered_channels(&guild.channels);
        if channels.is_empty() {
            list = list.child(div().px_3().child(widgets::supporting(t.no_channels, cx)));
        }
        for (channel, depth) in channels {
            list = list.child(self.render_channel_row(channel, depth, cx));
        }
        list.into_any_element()
    }

    fn render_channel_row(
        &mut self,
        channel: &Channel,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        if channel.kind == ChannelKind::Category {
            return div()
                .pt_3()
                .pl_3()
                .md_type(TypeRole::LabelMedium, cx)
                .text_color(colors.on_surface_variant)
                .child(channel.name.to_uppercase())
                .into_any_element();
        }
        let selected = self.channel.as_deref() == Some(&channel.id);
        let unread = self.session.unread(&channel.id);
        let id = channel.id.clone();
        let (bg, fg) = if selected {
            (colors.secondary_container, colors.on_secondary_container)
        } else if unread.unread || unread.mentions > 0 {
            (gpui::transparent_black(), colors.on_surface)
        } else {
            (gpui::transparent_black(), colors.on_surface_variant)
        };
        let glyph = if channel.kind == ChannelKind::Thread {
            Icon::Forum
        } else {
            Icon::Tag
        };
        let role = if unread.unread && !selected {
            div().md_type_emphasized(TypeRole::LabelLarge, cx)
        } else {
            div().md_type(TypeRole::BodyMedium, cx)
        };
        let mut row = div()
            .id(SharedString::from(format!("channel-{id}")))
            .h(px(36.0))
            .pl(px(12.0 + depth as f32 * 16.0))
            .pr_2()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .rounded_full()
            .bg(bg)
            .text_color(fg)
            .cursor_pointer()
            .hover(|style| style.bg(colors.surface_container_high))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(&id, window, cx)))
            .child(icon(glyph).size(px(18.0)).color(fg))
            .child(
                role.flex_1()
                    .min_w_0()
                    .truncate()
                    .child(channel.name.clone()),
            );
        if unread.mentions > 0 {
            row = row.child(widgets::badge(unread.mentions, cx));
        } else if unread.unread && !selected {
            row = row.child(div().size(px(8.0)).rounded_full().bg(colors.on_surface));
        }
        row.into_any_element()
    }

    fn render_user_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let settings = self.session.settings();
        let name = self.session.user_name().to_string();
        let status = if self.online {
            language_name(&settings.locale, t)
        } else {
            t.offline.to_string()
        };
        div()
            .flex_none()
            .m_2()
            .p_2()
            .rounded(px(20.0))
            .bg(colors.surface_container_high)
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                div()
                    .relative()
                    .child(widgets::avatar(&name, 36.0, cx))
                    .child(
                        div()
                            .absolute()
                            .bottom(px(-1.0))
                            .right(px(-1.0))
                            .size(px(12.0))
                            .rounded_full()
                            .border_2()
                            .border_color(colors.surface_container_high)
                            .bg(if self.online {
                                gpui::hsla(0.38, 0.6, 0.45, 1.0)
                            } else {
                                colors.outline
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .md_type(TypeRole::LabelLarge, cx)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .md_type(TypeRole::BodySmall, cx)
                            .text_color(if self.online {
                                colors.on_surface_variant
                            } else {
                                colors.error
                            })
                            .truncate()
                            .child(status),
                    ),
            )
            .child(
                IconButton::new("settings", Icon::Settings)
                    .label(t.settings)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dialog = Some(Dialog::settings(this, window, cx));
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The menu under the guild's name.
    pub(super) fn render_guild_menu(
        &mut self,
        guild: &GuildView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let owner = self.is_owner(guild);
        let mut items: Vec<(
            &'static str,
            Icon,
            Box<dyn Fn(&mut Self, &mut gpui::Window, &mut Context<Self>)>,
        )> = vec![
            (
                t.invite_people,
                Icon::PersonAdd,
                Box::new(|this, window, cx| {
                    this.dialog = Some(Dialog::invite(this, window, cx));
                }),
            ),
            (
                t.guild_languages,
                Icon::Translate,
                Box::new(|this, window, cx| {
                    this.open_languages(window, cx);
                }),
            ),
            (
                t.notifications,
                Icon::Notifications,
                Box::new(|this, window, cx| {
                    this.dialog = Some(Dialog::notifications(this, window, cx));
                }),
            ),
        ];
        if owner {
            items.push((
                t.create_channel,
                Icon::Add,
                Box::new(|this, window, cx| {
                    this.dialog = Some(Dialog::create_channel(this, window, cx));
                }),
            ));
            items.push((
                t.edit_guild,
                Icon::Settings,
                Box::new(|this, window, cx| {
                    this.dialog = Some(Dialog::edit_guild(this, window, cx));
                }),
            ));
        } else {
            items.push((
                t.leave_guild,
                Icon::Logout,
                Box::new(|this, window, cx| {
                    this.dialog = Some(Dialog::leave(this, window, cx));
                }),
            ));
        }

        let mut menu = div()
            .id("guild-menu")
            .occlude()
            .absolute()
            .top(px(60.0))
            .left(px(RAIL_WIDTH + 8.0))
            .w(px(SIDEBAR_WIDTH - 16.0))
            .py_2()
            .rounded(px(16.0))
            .bg(colors.surface_container)
            .md_elevation(2.0, colors.shadow)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.guild_menu = false;
                cx.notify();
            }));
        for (ix, (label, glyph, action)) in items.into_iter().enumerate() {
            menu = menu.child(
                div()
                    .id(("guild-menu-item", ix))
                    .h(px(44.0))
                    .px_4()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_container_highest))
                    .child(icon(glyph).size(px(20.0)).color(colors.on_surface_variant))
                    .child(div().md_type(TypeRole::LabelLarge, cx).child(label))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.guild_menu = false;
                        action(this, window, cx);
                        cx.notify();
                    })),
            );
        }
        menu.into_any_element()
    }

    pub(super) fn render_members(
        &mut self,
        guild: &GuildView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let viewer_is_owner = self.is_owner(guild);
        let me = self.session.user_id().to_string();
        let mut members = guild.members.clone();
        members.sort_by(|a, b| b.owner.cmp(&a.owner).then(a.name.cmp(&b.name)));

        let mut list = div()
            .id("members")
            .flex_none()
            .w(px(MEMBERS_WIDTH))
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_0p5()
            .p_2()
            .bg(colors.surface_container_low)
            .child(
                div()
                    .h(px(36.0))
                    .pl_3()
                    .flex()
                    .items_center()
                    .md_type(TypeRole::LabelMedium, cx)
                    .text_color(colors.on_surface_variant)
                    .child(format!("{} — {}", t.members, members.len())),
            );
        for member in members {
            let user_id = member.user_id.clone();
            let hovered = self.hovered_member.as_deref() == Some(&user_id);
            let mut row = div()
                .id(SharedString::from(format!("member-{user_id}")))
                .h(px(44.0))
                .px_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .rounded_full()
                .hover(|style| style.bg(colors.surface_container_high))
                .on_hover(cx.listener({
                    let user_id = user_id.clone();
                    move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hovered_member = Some(user_id.clone());
                        } else if this.hovered_member.as_deref() == Some(&user_id) {
                            this.hovered_member = None;
                        }
                        cx.notify();
                    }
                }))
                .child(widgets::avatar(&member.name, 32.0, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .md_type(TypeRole::LabelLarge, cx)
                                .truncate()
                                .child(member.name.clone()),
                        )
                        .when(member.owner, |column| {
                            column.child(
                                div()
                                    .md_type(TypeRole::LabelSmall, cx)
                                    .text_color(colors.primary)
                                    .child(t.owner),
                            )
                        }),
                );
            if viewer_is_owner && hovered && !member.owner && member.user_id != me {
                let kick_id = user_id.clone();
                let name = member.name.clone();
                let ban_name = member.name.clone();
                row = row
                    .child(
                        IconButton::new(
                            SharedString::from(format!("kick-{user_id}")),
                            Icon::PersonRemove,
                        )
                        .size(ButtonSize::XSmall)
                        .label(t.kick)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.dialog = Some(Dialog::remove_member(
                                    this, &kick_id, &name, false, window, cx,
                                ));
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        IconButton::new(SharedString::from(format!("ban-{user_id}")), Icon::Block)
                            .size(ButtonSize::XSmall)
                            .label(t.ban)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dialog = Some(Dialog::remove_member(
                                    this, &user_id, &ban_name, true, window, cx,
                                ));
                                cx.notify();
                            })),
                    );
            }
            list = list.child(row);
        }
        list.into_any_element()
    }
}

/// Channels in display order with their depth: channels without a category
/// first, then each category with its channels; threads under their channel.
fn ordered_channels(channels: &[Channel]) -> Vec<(&Channel, usize)> {
    let mut ordered = Vec::new();
    fn children<'a>(channels: &'a [Channel], parent: &'a str) -> impl Iterator<Item = &'a Channel> {
        channels
            .iter()
            .filter(move |channel| channel.parent_id.as_deref() == Some(parent))
    }
    let roots = channels
        .iter()
        .filter(|channel| channel.parent_id.is_none());
    let (categories, top): (Vec<&Channel>, Vec<&Channel>) =
        roots.partition(|channel| channel.kind == ChannelKind::Category);
    for channel in top {
        ordered.push((channel, 0));
        for thread in children(channels, &channel.id) {
            ordered.push((thread, 1));
        }
    }
    for category in categories {
        ordered.push((category, 0));
        for channel in children(channels, &category.id) {
            ordered.push((channel, 0));
            for thread in children(channels, &channel.id) {
                ordered.push((thread, 1));
            }
        }
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: &str, kind: ChannelKind, parent: Option<&str>) -> Channel {
        Channel {
            id: id.into(),
            guild_id: "g".into(),
            kind,
            name: id.into(),
            topic: None,
            parent_id: parent.map(Into::into),
            starter_message_id: None,
            created_at: 0,
            last_message_id: None,
            created_by: None,
        }
    }

    #[test]
    fn channels_come_before_categories_and_threads_follow_their_channel() {
        let channels = [
            channel("voice", ChannelKind::Category, None),
            channel("general", ChannelKind::Text, None),
            channel("music", ChannelKind::Text, Some("voice")),
            channel("plans", ChannelKind::Thread, Some("general")),
            channel("random", ChannelKind::Text, None),
        ];
        let order: Vec<(&str, usize)> = ordered_channels(&channels)
            .into_iter()
            .map(|(channel, depth)| (channel.id.as_str(), depth))
            .collect();
        assert_eq!(
            order,
            [
                ("general", 0),
                ("plans", 1),
                ("random", 0),
                ("voice", 0),
                ("music", 0)
            ]
        );
    }
}
