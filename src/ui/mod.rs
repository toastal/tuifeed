//! # Ui
//!
//! Ui for tuifeed

/**
 * MIT License
 *
 * tuifeed - Copyright (c) 2021 Christian Visintin
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */
mod components;
mod context;
mod lib;

use components::{ErrorPopup, GlobalListener};
pub use context::Context;

use crate::config::Config;
use lib::{FeedClient, FeedState, Kiosk};

use std::time::{Duration, Instant};
use tuirealm::{
    application::PollStrategy,
    event::{Key, KeyEvent, KeyModifiers},
    props::{PropPayload, PropValue},
    Application, AttrValue, Attribute, EventListenerCfg, NoUserEvent, Sub, SubClause,
    SubEventClause, Update,
};

use self::lib::FlatFeedState;

const FORCED_REDRAW_INTERVAL: Duration = Duration::from_millis(50);

/// ## Id
///
/// identifiers for components
#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum Id {
    GlobalListener,
    FeedList,
    ArticleList,
    ArticleTitle,
    ArticleDate,
    ArticleAuthors,
    ArticleSummary,
    ArticleLink,
    QuitPopup,
    ErrorPopup,
}

/// ## Msg
///
/// Messages produced by components
#[derive(Debug, PartialEq)]
pub enum Msg {
    ArticleBlur,
    ArticleChanged(usize),
    ArticleListBlur,
    CloseApp,
    CloseErrorPopup,
    CloseQuitPopup,
    FeedChanged(usize),
    FeedListBlur,
    FetchSource,
    FetchAllSources,
    GoReadArticle,
    OpenArticle,
    ShowQuitPopup,
    None,
}

pub struct Ui {
    context: Option<Context>,
    client: FeedClient,
    app: Application<Id, Msg, NoUserEvent>,
    kiosk: Kiosk,
    quit: bool,
    last_redraw: Instant,
    redraw: bool,
}

impl Ui {
    /// ### new
    ///
    /// Instantiates a new Ui
    pub fn new(context: Context, tick: u64) -> Self {
        let mut kiosk = Kiosk::default();
        for name in context.config().sources.keys() {
            kiosk.insert_feed(name, FeedState::Loading);
        }
        Self {
            context: Some(context),
            client: FeedClient::default(),
            app: Application::init(
                EventListenerCfg::default()
                    .default_input_listener(Duration::from_millis(tick))
                    .poll_timeout(Duration::from_millis(tick)),
            ),
            kiosk,
            quit: false,
            last_redraw: Instant::now(),
            redraw: false,
        }
    }

    /// ### run
    ///
    /// Main loop for Ui thread
    pub fn run(&mut self) {
        let _ = self.context_mut().terminal().enable_raw_mode();
        let _ = self.context_mut().terminal().enter_alternate_screen();
        let _ = self.context_mut().terminal().clear_screen();
        self.init_app();
        // Fetch sources once
        self.fetch_all_sources();
        // Main loop
        while !self.quit {
            match self.app.tick(PollStrategy::UpTo(3)) {
                Ok(messages) => {
                    if !messages.is_empty() {
                        self.force_redraw();
                    }
                    for msg in messages.into_iter() {
                        let mut msg = Some(msg);
                        while msg.is_some() {
                            msg = self.update(msg);
                        }
                    }
                }
                Err(err) => {
                    self.mount_error_popup(format!("Application error: {}", err));
                }
            }
            // Poll fetched sources
            self.poll_fetched_sources();
            // Check whether to force redraw
            self.check_force_redraw();
            // View
            self.view();
        }
        let _ = self.context_mut().terminal().disable_raw_mode();
        let _ = self.context_mut().terminal().leave_alternate_screen();
        let _ = self.context_mut().terminal().clear_screen();
    }

    // -- private

    /// ### run_tasks
    ///
    /// Run model tasks
    fn run_tasks(&mut self) {
        for task in self.model.get_tasks().into_iter() {
            match task {
                Task::FetchSource(name) => {
                    let uri = self.context().config().sources.get(&name).cloned();
                    if let Some(uri) = uri {
                        self.fetch_source(name.as_str(), uri.as_str())
                    }
                }
                Task::FetchSources => self.fetch_all_sources(),
                Task::ShowError(err) => self.mount_error_popup(err),
            }
        }
    }

    /// ### check_force_redraw
    ///
    /// Check whether should force redraw
    fn check_force_redraw(&mut self) {
        // If source are loading and at least 100ms has elapsed since last redraw...
        if self.client.running() && self.last_redraw.elapsed() >= FORCED_REDRAW_INTERVAL {
            self.force_redraw();
        }
    }

    fn context(&self) -> &Context {
        self.context.as_ref().unwrap()
    }

    fn context_mut(&mut self) -> &mut Context {
        self.context.as_mut().unwrap()
    }

    // -- source fetch

    /// ### fetch_all_sources
    ///
    /// Fetch all sources and update Ui
    #[allow(clippy::needless_collect)]
    fn fetch_all_sources(&mut self) {
        // Fetch sources
        let sources: Vec<(String, String)> = self
            .context()
            .config()
            .sources
            .iter()
            .map(|(name, uri)| (name.clone(), uri.clone()))
            .collect();
        for (name, uri) in sources.into_iter() {
            self.fetch_source(name.as_str(), uri.as_str());
        }
    }

    /// ### fetch_source
    ///
    /// Start a worker to fetch sources
    fn fetch_source(&mut self, name: &str, uri: &str) {
        self.client.fetch(name, uri);
        // Mark source as Loading
        self.kiosk.insert_feed(name, FeedState::Loading);
        self.update_feed_list(name, FlatFeedState::Loading);
        // Force redraw
        self.force_redraw();
    }

    /// ### poll_fetched_sources
    ///
    /// Get result for all fetched sources
    fn poll_fetched_sources(&mut self) {
        if let Some((name, result)) = self.client.poll() {
            // Adapt state
            let state = match result {
                Ok(feed) => FeedState::Success(feed),
                Err(err) => {
                    // Mount error and return err
                    self.mount_error_popup(format!(r#"Could not fetch feed "{}": {}"#, name, err));
                    FeedState::Error(err)
                }
            };
            // Update source
            let flat_state = FlatFeedState::from(&state);
            self.kiosk.insert_feed(name.as_str(), state);
            // Update feed list and initialize article
            self.update_feed_list(name.as_str(), flat_state);
            if self.is_article_list_empty() {
                self.init_article();
            }
            // Force redraw
            self.force_redraw();
        }
    }

    fn update_feed_list(&mut self, name: &str, state: FlatFeedState) {
        // Update item
        let state = match state {
            FlatFeedState::Error => components::lists::FEED_STATE_ERROR,
            FlatFeedState::Loading => components::lists::FEED_STATE_LOADING,
            FlatFeedState::Success => components::lists::FEED_STATE_SUCCESS,
        };
        let prop_value = AttrValue::Payload(PropPayload::Tup2((
            PropValue::Str(name.to_string()),
            PropValue::U8(state),
        )));
        assert!(self
            .app
            .attr(
                &Id::FeedList,
                Attribute::Custom(components::lists::FEED_LIST_PROP_ITEMS),
                prop_value
            )
            .is_ok());
    }

    // -- init

    /// ### init_article
    ///
    /// Initialize article list entries and article.
    /// This function should be called only if article list is empty
    fn init_article(&mut self) {
        if let Some(source) = self.sorted_sources().get(0) {
            if let Some(feed) = self.kiosk.get_feed(source.as_str()) {
                assert!(self
                    .app
                    .remount(
                        Id::ArticleList,
                        Box::new(View::get_article_list(feed, self.max_article_name_len())),
                        vec![]
                    )
                    .is_ok());
                // Mount first article
                if let Some(article) = feed.articles().next() {
                    let (authors, date, link, summary, title) = Model::get_article_view(article);
                    assert!(self
                        .app
                        .remount(Id::ArticleAuthors, Box::new(authors), vec![])
                        .is_ok());
                    assert!(self
                        .app
                        .remount(Id::ArticleDate, Box::new(date), vec![])
                        .is_ok());
                    assert!(self
                        .app
                        .remount(Id::ArticleLink, Box::new(link), vec![])
                        .is_ok());
                    assert!(self
                        .app
                        .remount(Id::ArticleSummary, Box::new(summary), vec![])
                        .is_ok());
                    assert!(self
                        .app
                        .remount(Id::ArticleTitle, Box::new(title), vec![])
                        .is_ok());
                }
            }
        }
    }

    /// ### is_article_list_empty
    ///
    /// Returns whether article list is empty
    fn is_article_list_empty(&self) -> bool {
        self.app
            .query(&Id::ArticleList, Attribute::Content)
            .ok()
            .flatten()
            .map(|x| x.unwrap_table().is_empty())
            .unwrap_or(true)
    }

    /// ### mount_error_popup
    ///
    /// Mount error and give focus to it
    fn mount_error_popup(&mut self, err: impl ToString) {
        assert!(self
            .app
            .remount(
                Id::ErrorPopup,
                Box::new(ErrorPopup::new(err.to_string())),
                vec![]
            )
            .is_ok());
        assert!(self.app.active(&Id::ErrorPopup).is_ok());
    }

    /// ### init_application
    ///
    /// Initialize application.
    /// Panics if it fails
    fn init_application(kiosk: &Kiosk, tick: u64) -> Application<Id, Msg, NoUserEvent> {
        let mut app = Application::init(
            EventListenerCfg::default()
                .default_input_listener(Duration::from_millis(tick))
                .poll_timeout(Duration::from_millis(tick)),
        );
        assert!(app
            .mount(Id::FeedList, Box::new(model.get_feed_list()), vec![])
            .is_ok());
        assert!(app
            .mount(
                Id::ArticleList,
                Box::new(Model::get_empty_article_list()),
                vec![]
            )
            .is_ok());
        assert!(app
            .mount(
                Id::GlobalListener,
                Box::new(GlobalListener::default()),
                Self::subs(),
            )
            .is_ok());
        assert!(app.active(&Id::FeedList).is_ok());
        app
    }

    /// ### subs
    ///
    /// global listener subscriptions
    fn subs() -> Vec<Sub<Id, NoUserEvent>> {
        vec![
            Sub::new(
                SubEventClause::Keyboard(KeyEvent {
                    code: Key::Esc,
                    modifiers: KeyModifiers::NONE,
                }),
                SubClause::Always,
            ),
            Sub::new(
                SubEventClause::Keyboard(KeyEvent {
                    code: Key::Char('r'),
                    modifiers: KeyModifiers::CONTROL,
                }),
                SubClause::Always,
            ),
            Sub::new(
                SubEventClause::Keyboard(KeyEvent {
                    code: Key::Char('r'),
                    modifiers: KeyModifiers::NONE,
                }),
                SubClause::Always,
            ),
        ]
    }

    // -- ex model funcs

    /// ### max_article_name_len
    ///
    /// Get max article name length for the article list
    fn max_article_name_len(&self) -> usize {
        (self.terminal_width() / 2) - 9 // 50 % - margin - 1
    }

    /// ### force_redraw
    ///
    /// Force the value of redraw to `true`
    pub fn force_redraw(&mut self) {
        self.redraw = true;
    }

    /// ### sorted_sources
    ///
    /// Get sorted sources from kiosk
    fn sorted_sources(&self) -> Vec<&String> {
        let mut sources = self.kiosk.sources();
        sources.sort();
        sources
    }

    /// ### terminal_width
    ///
    /// Get terminal width. If it fails to collect width, returns 65535
    fn terminal_width(&self) -> usize {
        self.context_mut()
            .terminal()
            .raw()
            .size()
            .map(|x| x.width as usize)
            .unwrap_or(u16::MAX as usize)
    }
}
