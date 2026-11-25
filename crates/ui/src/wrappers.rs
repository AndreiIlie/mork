//! Scrolling, focus and keys are the window's (1.5.2): the app only passes messages on.
//! The conversation's scroll runs once per ask, held in px (5.1.5, fac-42).

use factory_types::CardId;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::operation::{focusable, scrollable};
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::widget::text_editor::{Action, Edit};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, keyboard};

use crate::app::{Message, ScrollOp};
use crate::keys;

#[derive(Debug, Clone, Copy)]
enum Role<'a> {
    // Runs the scroll asked under each number once, on the scrollable with this id.
    Scroll { target: &'static str, asked: Option<(u64, ScrollOp)> },
    Root { input: &'static str },
    FullHeight,
    Summary(&'a CardId),
}

#[derive(Debug, Default)]
struct State {
    ran: u64,
    // It has seen an event: the window is up.
    started: bool,
    // A scroll to a share ran, not yet held in px.
    hold: bool,
}

struct Wrapper<'a> {
    content: Element<'a, Message>,
    role: Role<'a>,
}

/// The conversation's scrollable, scrolled as the window asks.
pub fn scrolled<'a>(
    content: impl Into<Element<'a, Message>>,
    target: &'static str,
    asked: Option<(u64, ScrollOp)>,
) -> Element<'a, Message> {
    Element::new(Wrapper { content: content.into(), role: Role::Scroll { target, asked } })
}

/// The whole window, with its keys and its input's focus held.
pub fn root<'a>(content: impl Into<Element<'a, Message>>, input: &'static str) -> Element<'a, Message> {
    Element::new(Wrapper { content: content.into(), role: Role::Root { input } })
}

/// The content's full natural height, unconstrained by its parent's available height.
pub fn full_height<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Wrapper { content: content.into(), role: Role::FullHeight })
}

/// Keeps a summary's scroll position within one room.
pub(crate) fn room_summary<'a>(content: impl Into<Element<'a, Message>>, card: &'a CardId) -> Element<'a, Message> {
    Element::new(Wrapper { content: content.into(), role: Role::Summary(card) })
}

struct Reveal {
    target: Id,
    top: f32,
    bottom: f32,
}

impl Operation for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn scrollable::Scrollable,
    ) {
        if id != Some(&self.target) {
            return;
        }
        let y = if self.top < translation.y {
            self.top
        } else if self.bottom > translation.y + bounds.height {
            self.bottom - bounds.height
        } else {
            return;
        };
        state.scroll_to(scrollable::AbsoluteOffset { x: None, y: Some(y.max(0.0)) });
    }
}

// A share held as the same offset in px, measured from the bottom as the conversation anchors it,
// once its content is longer than its space.
struct Hold {
    target: Id,
    held: bool,
}

impl Operation for Hold {
    // The wrapper's content is the scrollable itself: nothing under it to visit.
    fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation)) {}

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        _translation: Vector,
        state: &mut dyn scrollable::Scrollable,
    ) {
        if id == Some(&self.target) && content_bounds.height > bounds.height {
            state.scroll_by(scrollable::AbsoluteOffset { x: 0.0, y: 0.0 }, bounds, content_bounds);
            self.held = true;
        }
    }
}

fn scroll(target: &'static str, op: ScrollOp) -> Box<dyn Operation> {
    match op {
        ScrollOp::Show { top, bottom } => Box::new(Reveal { target: Id::new(target), top, bottom }),
        ScrollOp::By(y) => Box::new(scrollable::scroll_by(Id::new(target), scrollable::AbsoluteOffset { x: 0.0, y })),
        ScrollOp::To(y) => {
            Box::new(scrollable::snap_to(Id::new(target), scrollable::RelativeOffset { x: None, y: Some(y) }))
        }
    }
}

fn typed(text: Option<&str>, m: keyboard::Modifiers) -> Option<&str> {
    let plain = !m.control() && !m.alt() && !m.logo();
    text.filter(|t| plain && !t.is_empty() && !t.chars().any(char::is_control))
}

impl Wrapper<'_> {
    fn run(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    // A fresh wrapper (the conversation drawn again) brings back a scroll to a share, but not one by a step,
    // which was taken from where the old one stood.
    fn scroll_asked(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let Role::Scroll { target, asked: Some((n, op)) } = self.role else { return };
        let state = tree.state.downcast_mut::<State>();
        if state.ran == n {
            return;
        }
        let fresh = state.ran == 0;
        state.ran = n;
        if fresh && matches!(op, ScrollOp::By(_)) {
            return;
        }
        state.hold |= matches!(op, ScrollOp::To(_));
        self.run(tree, layout, renderer, scroll(target, op).as_mut());
        shell.request_redraw();
    }

    // Before the content sees the event, so the sensor's ask for a page finds it held already.
    fn hold_share(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) {
        let Role::Scroll { target, .. } = self.role else { return };
        if !tree.state.downcast_ref::<State>().hold {
            return;
        }
        let mut op = Hold { target: Id::new(target), held: false };
        self.run(tree, layout, renderer, &mut op);
        tree.state.downcast_mut::<State>().hold = !op.held;
    }

    // After the content saw the event: a key nobody took is the window's, or typed into the
    // input; and the input takes the focus at the start, after a click and when typed into.
    fn hold(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let Role::Root { input } = self.role else { return };
        let state = tree.state.downcast_mut::<State>();
        let mut focus = !std::mem::replace(&mut state.started, true);
        focus |= matches!(event, Event::Mouse(mouse::Event::ButtonReleased(_)));
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, text, .. }) = event
            && !shell.is_event_captured()
        {
            if let Some(k) = keys::window_key(key, *modifiers, false) {
                shell.publish(Message::Key(k));
                shell.capture_event();
            } else if let Some(t) = typed(text.as_deref(), *modifiers) {
                for c in t.chars() {
                    shell.publish(Message::Edit(Action::Edit(Edit::Insert(c))));
                }
                shell.capture_event();
                focus = true;
            }
        }
        if focus {
            self.run(tree, layout, renderer, &mut focusable::focus(Id::new(input)));
        }
    }
}

impl Widget<Message, Theme, Renderer> for Wrapper<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let limits = if matches!(self.role, Role::FullHeight) {
            layout::Limits::new(Size::new(limits.min().width, 0.0), Size::new(limits.max().width, f32::INFINITY))
        } else {
            *limits
        };
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, &limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn tag(&self) -> tree::Tag {
        match self.role {
            Role::Summary(_) => tree::Tag::of::<CardId>(),
            _ => tree::Tag::of::<State>(),
        }
    }

    fn state(&self) -> tree::State {
        match self.role {
            Role::Summary(card) => tree::State::new(card.clone()),
            _ => tree::State::new(State::default()),
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        if let Role::Summary(card) = self.role
            && tree.state.downcast_ref::<CardId>() != card
        {
            tree.state = self.state();
            tree.children = self.children();
        } else {
            tree.diff_children(std::slice::from_ref(&self.content));
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.run(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.scroll_asked(tree, layout, renderer, shell);
        self.hold_share(tree, layout, renderer);
        let content = self.content.as_widget_mut();
        content.update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
        self.hold(tree, event, layout, renderer, shell);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}
