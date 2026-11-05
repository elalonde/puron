//! The libcosmic widgets built from the window plan: libcosmic's settings
//! section and item widgets with their own spacing and padding; one
//! column of the six sections in a vertical scroll area.
//!
//! Ids, used by the tests that measure widget bounds. Each is a container
//! with no padding around one widget, so it adds nothing to the layout:
//! `item-<N>` around the item row (N is the item number, from 1),
//! `control-<N>` around its control, `section-<N>` around each section,
//! `scroll` on the scroll area, and `minus`, `input`, `plus` on the
//! number field's parts.

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{
    button, container, dropdown, icon, row, scrollable, settings, text, text_input, toggler,
    tooltip, Id,
};
use cosmic::Element;

use crate::window::{Control, Event, Plan, INPUT_WIDTH, MINUS_ICON, PLUS_ICON};

/// The id `name` of a measured widget.
pub fn id(name: impl Into<String>) -> Id {
    Id::new(name.into())
}

fn number<'a>(text_shown: &'a str, minus: bool, plus: bool) -> Element<'a, Event> {
    let space = cosmic::theme::spacing().space_xxs;
    let minus =
        button::icon(icon::from_name(MINUS_ICON)).on_press_maybe(minus.then_some(Event::Minus));
    let plus = button::icon(icon::from_name(PLUS_ICON)).on_press_maybe(plus.then_some(Event::Plus));
    let input = text_input("", text_shown)
        .id(id("input"))
        .width(Length::Fixed(INPUT_WIDTH))
        .on_input(Event::Typed)
        .on_submit(|_| Event::Commit)
        .on_unfocus(Event::Commit);
    row::with_children(vec![
        container(minus).id(id("minus")).into(),
        input.into(),
        container(plus).id(id("plus")).into(),
    ])
    .spacing(space)
    .align_y(Alignment::Center)
    .into()
}

fn control<'a>(n: usize, c: &'a Control) -> Element<'a, Event> {
    let widget: Element<'a, Event> = match c {
        Control::Number {
            text, minus, plus, ..
        } => number(text, *minus, *plus),
        Control::Switch(on) => toggler(*on)
            .on_toggle(move |on| Event::Switch { item: n, on })
            .into(),
        Control::List { items, shown } => {
            dropdown(items.as_slice(), Some(*shown), Event::Choose).into()
        }
    };
    container(widget).id(id(format!("control-{n}"))).into()
}

/// The content under the header bar for `plan`.
pub fn view<'a>(plan: &'a Plan) -> Element<'a, Event> {
    let mut n = 0;
    let sections = plan
        .sections
        .iter()
        .enumerate()
        .map(|(s, sec)| {
            let items = sec.items.iter().map(|it| {
                n += 1;
                let row =
                    settings::item::builder(it.label.as_str()).control(control(n, &it.control));
                let item: Element<'a, Event> = container(row).id(id(format!("item-{n}"))).into();
                match &it.tooltip {
                    // libcosmic's tooltip around the whole item, at its
                    // default place.
                    Some((_, tip)) => {
                        tooltip(item, text(tip.as_str()), tooltip::Position::default()).into()
                    }
                    None => item,
                }
            });
            let section: Element<'a, Event> = settings::section()
                .title(sec.heading.as_str())
                .extend(items.collect::<Vec<_>>())
                .into();
            container(section)
                .id(id(format!("section-{}", s + 1)))
                .into()
        })
        .collect();
    scrollable(settings::view_column(sections))
        .id(id("scroll"))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
