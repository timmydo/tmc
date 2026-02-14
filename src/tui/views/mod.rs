pub mod email_list;
pub mod email_view;
pub mod mailbox_list;

use super::input::Key;
use super::screen::Terminal;
use crate::backend::BackendResponse;
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
    /// Handle a response from the backend thread.
    /// Returns true if the view consumed the response and should re-render.
    fn on_response(&mut self, response: &BackendResponse) -> bool;
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

    /// Route a backend response to the current view.
    /// Returns true if a re-render is needed.
    pub fn handle_response(&mut self, response: &BackendResponse) -> bool {
        if let Some(view) = self.views.last_mut() {
            view.on_response(response)
        } else {
            false
        }
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
}
