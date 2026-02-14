pub mod mailbox_list;

use super::input::Key;
use super::screen::Terminal;
use std::io;

pub enum ViewAction {
    Continue,
    Push(Box<dyn View>),
    Pop,
    Quit,
}

pub trait View {
    fn render(&self, term: &mut Terminal) -> io::Result<()>;
    fn handle_key(&mut self, key: Key) -> ViewAction;
}

pub struct ViewStack {
    views: Vec<Box<dyn View>>,
}

impl ViewStack {
    pub fn new(initial: Box<dyn View>) -> Self {
        ViewStack {
            views: vec![initial],
        }
    }

    pub fn render_current(&self, term: &mut Terminal) -> io::Result<()> {
        if let Some(view) = self.views.last() {
            view.render(term)?;
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: Key) -> Option<ViewAction> {
        self.views.last_mut().map(|view| view.handle_key(key))
    }

    pub fn push(&mut self, view: Box<dyn View>) {
        self.views.push(view);
    }

    pub fn pop(&mut self) -> bool {
        if self.views.len() > 1 {
            self.views.pop();
            true
        } else {
            false
        }
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }
}
