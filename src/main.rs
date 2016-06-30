#![feature(io)]
extern crate libc;
extern crate ncurses;
extern crate getopts;
mod util;
mod item;
mod reader;
mod input;
mod matcher;
mod event;
mod model;
mod score;
mod orderedvec;
mod curses;
mod query;

use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::sync::mpsc::channel;
use std::mem;
use std::ptr;
use util::eventbox::EventBox;

use ncurses::*;
use event::Event;
use input::Input;
use reader::Reader;
use matcher::Matcher;
use model::Model;
use libc::{sigemptyset, sigaddset, sigwait, pthread_sigmask};
use curses::{ColorTheme, Curses};
use getopts::Options;
use std::env;

fn main() {

    // option parsing
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt("b", "bind", "comma seperated keybindings", "ctrl-j:accept,ctrl-k:kill-line");
    opts.optflag("h", "help", "print this help menu");

    let options = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };

    // print help message
    if options.opt_present("h") {
        print_usage(&program, opts);
        return
    }

    let theme = ColorTheme::new();
    let mut curse = Curses::new();
    curse.init(Some(&theme), false, false);

    let eb = Arc::new(EventBox::new());
    let (tx_matched, rx_matched) = channel();
    let eb_model = eb.clone();
    let mut model = Model::new(eb_model, curse);

    let eb_matcher = Arc::new(EventBox::new());
    let eb_matcher_clone = eb_matcher.clone();
    let eb_clone_matcher = eb.clone();
    let items = model.items.clone();
    let mut matcher = Matcher::new(items, tx_matched, eb_matcher_clone, eb_clone_matcher);

    let eb_clone_reader = eb.clone();
    let items = model.items.clone();
    let mut reader = Reader::new(Some(&"find ."), eb_clone_reader, items);


    let eb_clone_input = eb.clone();
    let mut input = Input::new(eb_clone_input);
    input.parse_keymap(options.opt_str("b"));

    // register terminal resize event, `pthread_sigmask` should be run before any thread.
    let mut sigset = unsafe {mem::uninitialized()};
    unsafe {
        sigemptyset(&mut sigset);
        sigaddset(&mut sigset, libc::SIGWINCH);
        pthread_sigmask(libc::SIG_SETMASK, &mut sigset, ptr::null_mut());
    }

    let eb_clone_resize = eb.clone();
    thread::spawn(move || {
        // listen to the resize event;
        loop {
            let mut sig = 0;
            let _errno = unsafe {sigwait(&mut sigset, &mut sig)};
            eb_clone_resize.set(Event::EvResize, Box::new(true));
        }
    });

    // start running
    thread::spawn(move || {
        reader.run();
    });

    thread::spawn(move || {
        matcher.run();
    });

    thread::spawn(move || {
        input.run();
    });

    'outer: loop {
        let mut need_refresh = true;
        for (e, val) in eb.wait() {
            match e {
                Event::EvReaderNewItem | Event::EvReaderFinished => {
                    eb_matcher.set(Event::EvMatcherNewItem, Box::new(true));
                }

                Event::EvMatcherUpdateProcess => {
                    let (matched, total, processed) : (u64, u64, u64) = *val.downcast().unwrap();
                    model.update_process_info(matched, total, processed);

                    while let Ok(matched_item) = rx_matched.try_recv() {
                        model.push_item(matched_item);
                    }
                }

                Event::EvMatcherStart => {
                    while let Ok(_) = rx_matched.try_recv() {}
                    model.clear_items();
                    eb_matcher.set(Event::EvMatcherStartReceived, Box::new(true));
                    need_refresh = false;
                }

                Event::EvMatcherEnd => {
                    // do nothing
                }

                Event::EvQueryChange => {
                    eb_matcher.set(Event::EvMatcherResetQuery, val);
                }

                Event::EvInputInvalid => {
                    // ignore
                }

                Event::EvInputKey => {
                    // ignore for now
                }

                Event::EvResize => { model.resize(); }

                Event::EvActAddChar => {
                    let ch: char = *val.downcast().unwrap();
                    model.act_add_char(ch);
                }
                // Actions

                Event::EvActAbort => { break 'outer; }
                Event::EvActAccept => {
                    // break out of the loop and output the selected item.
                    if model.get_num_selected() <= 0 { model.act_toggle_select(Some(true)); }
                    break 'outer;
                }
                Event::EvActBackwardChar => { model.act_backward_char(); }
                Event::EvActBackwardDeleteChar => { model.act_backward_delete_char(); }
                Event::EvActBackwardKillWord => {model.act_backward_kill_word();}
                Event::EvActBackwardWord => {model.act_backward_word();}
                Event::EvActBeginningOfLine => {model.act_beginning_of_line();}
                Event::EvActCancel => {}
                Event::EvActClearScreen => {}
                Event::EvActDeleteChar => {model.act_delete_char();}
                Event::EvActDeleteCharEOF => {model.act_delete_char();}
                Event::EvActDeselectAll => {}
                Event::EvActDown => { model.act_move_line_cursor(1); }
                Event::EvActEndOfLine => {model.act_end_of_line();}
                Event::EvActForwardChar => {model.act_forward_char();}
                Event::EvActForwardWord => {model.act_forward_word();}
                Event::EvActIgnore => {}
                Event::EvActKillLine => {model.act_kill_line();}
                Event::EvActKillWord => {model.act_kill_word();}
                Event::EvActNextHistory => {}
                Event::EvActPageDown => {}
                Event::EvActPageUp => {}
                Event::EvActPreviousHistory => {}
                Event::EvActSelectAll => {}
                Event::EvActToggle => {}
                Event::EvActToggleAll => {}
                Event::EvActToggleDown => {
                    model.act_toggle_select(None);
                    model.act_move_line_cursor(1);
                }
                Event::EvActToggleIn => {}
                Event::EvActToggleOut => {}
                Event::EvActToggleSort => {}
                Event::EvActToggleUp => {}
                Event::EvActUnixLineDiscard => {model.act_line_discard();}
                Event::EvActUnixWordRubout => {model.act_backward_kill_word();}
                Event::EvActUp => { model.act_move_line_cursor(-1); }
                Event::EvActYank => {}

                _ => {
                    printw(format!("{}\n", e as i32).as_str());
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
        model.display();
        if need_refresh {
            model.refresh();
        }
    };

    endwin();
    model.output();
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}
