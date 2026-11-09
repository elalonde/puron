//! Draws the settings window from a `window::Plan`.
//!
//! Each section of the plan becomes a libcosmic settings section, and
//! each item a settings item row, with libcosmic's own spacing and
//! padding. The sections (five, one per entry of `window::SECTIONS`)
//! stand in one column inside a vertical scroll area.
//!
//! Some widgets carry an id so that the tests can find them and measure
//! their bounds. `item-<N>` (N is the item number, from 1), `control-<N>`,
//! `section-<N>`, `minus` and `plus` are containers with no padding around
//! the item row, its control, the section and the number field's two
//! buttons, so they add nothing to the layout. `scroll` is the scroll
//! area itself, `input` the number field's text input, and `notice` the
//! line above the scroll area that reports a failed store.

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{
    button, column, container, dropdown, icon, row, scrollable, settings, text, text_input,
    toggler, tooltip, Id,
};
use cosmic::Element;

use crate::window::{Control, Event, Plan, INPUT_WIDTH, MINUS_ICON, PLUS_ICON};

/// The widget id with the name `name`, as the tests look it up.
pub fn id(name: impl Into<String>) -> Id {
    Id::new(name.into())
}

/// The number field: a − button, a text input and a + button in a row. A
/// button that is not enabled has no press message, so libcosmic draws it
/// disabled. Typing only records the text; Enter or leaving the field
/// commits it.
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

/// The control of item `n`, wrapped in its `control-<n>` container.
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

/// The window's content below the header bar, drawn from `plan`.
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
                    // The tooltip covers the whole item row, at
                    // libcosmic's default position.
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
    let scroll: Element<'a, Event> = scrollable(settings::view_column(sections))
        .id(id("scroll"))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    match &plan.notice {
        // The notice takes one line at the top and the scroll area the
        // rest; without it the scroll area fills the window as before.
        Some(notice) => column::with_children(vec![
            container(text(notice.as_str())).id(id("notice")).into(),
            scroll,
        ])
        .into(),
        None => scroll,
    }
}
