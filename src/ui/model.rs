//! # Model
//!
//! Application model

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
use super::components::{
    ArticleAuthors, ArticleDate, ArticleLink, ArticleList, ArticleSummary, ArticleTitle, FeedList,
    QuitPopup,
};
use super::lib::FeedState;
use super::{Id, Kiosk, Msg, Task};

use crate::feed::{Article, Feed};
use crate::helpers::open as open_helpers;
use crate::helpers::strings as str_helpers;
use crate::helpers::ui as ui_helpers;
use crate::Config;

use std::time::{Duration, Instant};
use tuirealm::terminal::TerminalBridge;
use tuirealm::tui::layout::{Constraint, Direction, Layout};
use tuirealm::tui::widgets::Clear;
use tuirealm::{Application, AttrValue, Attribute, NoUserEvent, State, StateValue, Update, View};

pub struct Model {
    kiosk: Kiosk,
    quit: bool,
    last_redraw: Instant,
    redraw: bool,
    terminal: TerminalBridge,
}

impl Model {
    /// ### view
    ///
    /// View function to render the view
    pub fn view(&mut self, app: &mut Application<Id, Msg, NoUserEvent>) {
        if self.redraw {
            self.redraw = false;
            self.last_redraw = Instant::now();
            assert!(self
                .terminal
                .raw_mut()
                .draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .margin(1)
                        .constraints(
                            [
                                Constraint::Percentage(50), // Lists
                                Constraint::Percentage(50), // Article
                            ]
                            .as_ref(),
                        )
                        .split(f.size());

                    // Render layout only if kiosk has been initialized
                    // -- list
                    let list_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .horizontal_margin(2)
                        .constraints(
                            [Constraint::Percentage(30), Constraint::Percentage(70)].as_ref(),
                        )
                        .split(chunks[0]);
                    app.view(&Id::FeedList, f, list_chunks[0]);
                    app.view(&Id::ArticleList, f, list_chunks[1]);
                    // -- article
                    let article_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [
                                Constraint::Length(3), // Title
                                Constraint::Length(1), // Authors + date
                                Constraint::Min(6),    // Summary
                                Constraint::Length(1), // Link
                            ]
                            .as_ref(),
                        )
                        .split(chunks[1]);
                    let second_article_row = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(
                            [Constraint::Percentage(60), Constraint::Percentage(40)].as_ref(),
                        )
                        .split(article_chunks[1]);
                    app.view(&Id::ArticleTitle, f, article_chunks[0]);
                    app.view(&Id::ArticleAuthors, f, second_article_row[0]);
                    app.view(&Id::ArticleDate, f, second_article_row[1]);
                    app.view(&Id::ArticleSummary, f, article_chunks[2]);
                    app.view(&Id::ArticleLink, f, article_chunks[3]);
                    // -- popups
                    if app.mounted(&Id::QuitPopup) {
                        let popup = ui_helpers::draw_area_in(f.size(), 30, 10);
                        f.render_widget(Clear, popup);
                        app.view(&Id::QuitPopup, f, popup);
                    } else if app.mounted(&Id::ErrorPopup) {
                        let popup = ui_helpers::draw_area_in(f.size(), 50, 15);
                        f.render_widget(Clear, popup);
                        app.view(&Id::ErrorPopup, f, popup);
                    }
                })
                .is_ok());
        }
    }

    /// ### update_article_view
    ///
    /// Update article into the view
    pub fn get_article_view(
        article: &Article,
    ) -> (
        ArticleAuthors,
        ArticleDate,
        ArticleLink,
        ArticleSummary,
        ArticleTitle,
    ) {
        (
            ArticleAuthors::new(article.authors.as_ref()),
            ArticleDate::new(article.date),
            ArticleLink::new(article.url.as_str()),
            ArticleSummary::new(article.summary.as_str()),
            ArticleTitle::new(article.title.as_deref().unwrap_or("")),
        )
    }

    /// ### update_article_list
    ///
    /// Update the current article list
    pub fn get_article_list(feed: &Feed, max_title_len: usize) -> ArticleList {
        let articles: Vec<String> = feed
            .articles()
            .map(|x| {
                x.title
                    .as_ref()
                    .map(|x| str_helpers::elide_string_at(x.as_str(), max_title_len))
            })
            .flatten()
            .collect();
        ArticleList::new(articles.as_slice())
    }

    /// ### get_empty_article_list
    ///
    /// Returns an empty article list component
    pub fn get_empty_article_list() -> ArticleList {
        ArticleList::new(&[])
    }

    /// ### get_feed_list
    ///
    /// Get feed list component
    pub fn get_feed_list(&self) -> FeedList {
        let mut sources = self.kiosk.get_state();
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        FeedList::new(sources)
    }

    /// ### view_quit
    ///
    /// Mount quit popup
    fn mount_quit(&self, view: &mut View<Id, Msg, NoUserEvent>) {
        assert!(view
            .remount(Id::QuitPopup, Box::new(QuitPopup::default()))
            .is_ok());
        assert!(view.active(&Id::QuitPopup).is_ok());
    }

    /// ### terminal_width
    ///
    /// Get terminal width. If it fails to collect width, returns 65535
    fn terminal_width(&self) -> usize {
        self.terminal
            .raw()
            .size()
            .map(|x| x.width as usize)
            .unwrap_or(u16::MAX as usize)
    }

    /// ### update_article
    ///
    /// Update article into view by index
    fn update_article(&self, view: &mut View<Id, Msg, NoUserEvent>, article: usize) {
        if let Some(feed) = self.get_selected_feed(view) {
            if let Some(article) = feed.articles().nth(article) {
                let (authors, date, link, summary, title) = Self::get_article_view(article);
                assert!(view.remount(Id::ArticleAuthors, Box::new(authors)).is_ok());
                assert!(view.remount(Id::ArticleDate, Box::new(date)).is_ok());
                assert!(view.remount(Id::ArticleLink, Box::new(link)).is_ok());
                assert!(view.remount(Id::ArticleSummary, Box::new(summary)).is_ok());
                assert!(view.remount(Id::ArticleTitle, Box::new(title)).is_ok());
            }
        }
    }

    /// ### get_selected_feed
    ///
    /// Get currently selected feed
    fn get_selected_feed(&self, view: &mut View<Id, Msg, NoUserEvent>) -> Option<&Feed> {
        if let Some(feed) = self.get_selected_feed_name(view) {
            Some(self.kiosk.get_feed(feed.as_str()).unwrap())
        } else {
            None
        }
    }

    /// ### get_selected_feed_name
    ///
    /// Get currently selected feed name
    fn get_selected_feed_name(&self, view: &mut View<Id, Msg, NoUserEvent>) -> Option<String> {
        if let State::One(StateValue::Usize(feed)) = view.state(&Id::FeedList).ok().unwrap() {
            Some((*self.sorted_sources().get(feed).unwrap()).clone())
        } else {
            None
        }
    }
}

impl Update<Id, Msg, NoUserEvent> for Model {
    fn update(&mut self, view: &mut View<Id, Msg, NoUserEvent>, msg: Option<Msg>) -> Option<Msg> {
        self.redraw = true;
        match msg.unwrap_or(Msg::None) {
            Msg::ArticleBlur => {
                assert!(view.active(&Id::ArticleList).is_ok());
            }
            Msg::ArticleChanged(article) => {
                self.update_article(view, article);
            }
            Msg::ArticleListBlur => {
                assert!(view.active(&Id::FeedList).is_ok());
            }
            Msg::CloseApp => {
                self.quit = true;
            }
            Msg::CloseErrorPopup => {
                let _ = view.umount(&Id::ErrorPopup);
            }
            Msg::CloseQuitPopup => {
                let _ = view.umount(&Id::QuitPopup);
            }
            Msg::FeedChanged(feed) => {
                let feed = &(*self.sorted_sources().get(feed).unwrap()).clone();
                if let Some(feed) = self.kiosk.get_feed(feed.as_str()) {
                    let articles = Self::get_article_list(feed, self.max_article_name_len());
                    assert!(view.remount(Id::ArticleList, Box::new(articles)).is_ok());
                    // Then load the first article of feed
                    self.update_article(view, 0);
                }
            }
            Msg::FeedListBlur => {
                assert!(view.active(&Id::ArticleList).is_ok());
            }
            Msg::FetchSource => {
                if let Some(name) = self.get_selected_feed_name(view) {
                    self.task(Task::FetchSource(name))
                }
            }
            Msg::FetchAllSources => {
                self.task(Task::FetchSources);
            }
            Msg::GoReadArticle => {
                let _ = view.active(&Id::ArticleSummary);
            }
            Msg::OpenArticle => {
                if let Ok(Some(AttrValue::String(url))) =
                    view.query(&Id::ArticleLink, Attribute::Text)
                {
                    if let Err(err) = open_helpers::open_link(url.as_str()) {
                        self.task(Task::ShowError(err));
                    }
                }
            }
            Msg::ShowQuitPopup => {
                self.mount_quit(view);
            }
            Msg::None => {}
        }
        None
    }
}
