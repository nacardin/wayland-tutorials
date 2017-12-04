#[macro_use]
extern crate wayland_client;
extern crate tempfile;
extern crate byteorder;
extern crate rand;

use byteorder::{NativeEndian, WriteBytesExt};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, RwLock};
use rand::{ThreadRng, Rng};
use wayland_client::EnvHandler;
use wayland_client::protocol::{wl_compositor, wl_pointer, wl_seat, wl_shell, wl_shell_surface,
                               wl_shm, wl_keyboard, wl_surface, wl_callback};

// buffer (and window) width and height
const BUF_X: u32 = 640;
const BUF_Y: u32 = 480;

// get references to wayland globals
wayland_env!(
    WaylandEnvironment,
    compositor: wl_compositor::WlCompositor,
    seat: wl_seat::WlSeat,
    shell: wl_shell::WlShell,
    shm: wl_shm::WlShm
);

// represents white rectangle which moves based on user input
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

// object we will pass around between draw loop and user input handlers
struct AppState {
    rect: Rect,
    tmp_file: File
}

// Atomic reference cell and reader-writer lock to safely share AppState
type ArcRwlAppState = Arc<RwLock<AppState>>;

impl AppState {
    fn new(tmp_file: File) -> ArcRwlAppState {
        Arc::new(RwLock::new(AppState {
            rect: Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 50,
            },
            tmp_file: tmp_file
        }))
    }
}

fn main() {

    // Connect to wayland server
    let (display, mut event_queue) =
        wayland_client::default_connect().expect("Cannot connect to wayland server");

    let registry = display.get_registry();

    let environment_state_token =
        EnvHandler::<WaylandEnvironment>::init(&mut event_queue, &registry);

    event_queue
        .sync_roundtrip()
        .expect("Failed to sync with wayland server");

    // creates a tempfile to use as a shared buffer beetween this app and the wayland compositor
    let mut tmp = tempfile::tempfile().expect("Unable to create a tempfile.");

    let env = event_queue
        .state()
        .get(&environment_state_token)
        .clone_inner()
        .unwrap();

    let surface = env.compositor.create_surface();
    let shell_surface = env.shell.get_shell_surface(&surface);

    let pool = env.shm
        .create_pool(tmp.as_raw_fd(), (BUF_X * BUF_Y * 4) as i32);

    let buffer = pool.create_buffer(
        0,
        BUF_X as i32,
        BUF_Y as i32,
        (BUF_X * 4) as i32,
        wl_shm::Format::Argb8888,
    ).expect("The pool cannot be already dead");

    // make our surface as a toplevel one
    shell_surface.set_toplevel();
    
    let pointer = env.seat
        .get_pointer()
        .expect("Seat cannot be already destroyed.");

    let keyboard = env.seat
        .get_keyboard()
        .expect("Seat cannot be already destroyed.");

    let app_state = AppState::new(tmp);

    event_queue.register(&shell_surface, create_shell_surface_event_hander(), ());
    event_queue.register(&pointer, create_pointer_event_hander(), app_state.clone());
    event_queue.register(&keyboard, create_keyboard_event_hander(), app_state.clone());

    // use random number generator to create background noise, to know when a frame has been rendered
    // let mut rng = rand::thread_rng();

draw(&app_state);

    // infinite loop to draw and receive user input
    loop {

        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, BUF_X as i32, BUF_Y as i32).expect("Failed to damage buffer");
        surface.commit();

        display.flush().expect("Error flushing display");

        let frame_cb = surface.frame().expect("unable to request frame hint");
        event_queue.register(&frame_cb, create_callback_event_hander(), app_state.clone());

        event_queue.dispatch().expect("Event queue dispatch failed");
    }
}

// application draw logic to run on each frame
fn draw(app_state: &ArcRwlAppState) {
    use std::io::{Seek, SeekFrom};

    // get AppState from lock, using read() as to not block other readers
    let readable_app_state = app_state.read().unwrap();

    let mut tmp = &readable_app_state.tmp_file;

    println!(
        "Rect moved to ({},{}).",
        readable_app_state.rect.x,
        readable_app_state.rect.y
    );

    // go to start of buffer
    tmp.seek(SeekFrom::Start(0)).unwrap();

    // check if pixel in within rectangle
    fn is_coords_in_rect(rect: &Rect, i: u32, j: u32) -> bool {
        i > rect.x && i < rect.x + rect.w && j > rect.y && j < rect.y + rect.h
    }

    // let bg_color = rng.gen::<u8>() as u32;
    let bg_color = 0u8;

    let mut pixels: Vec<u8> = Vec::new();
    pixels.reserve_exact(BUF_X as usize * BUF_Y as usize);

    // draw random pixels into buffer, white pixel inside Rect based on current app state
    for i in 0..(BUF_X * BUF_Y) {
        let x = (i % BUF_X) as u32;
        let y = (i / BUF_Y) as u32;

        let mut r = bg_color;
        let mut g = bg_color;
        let mut b = bg_color;

        if is_coords_in_rect(&readable_app_state.rect, x, y) {
            r = 255;
            g = 255;
            b = 255;
        }
        // pixels.push((0xFF << 24) + (r << 16) + (g << 8) + b);
        pixels.push(0xFF);
        pixels.push(r);
        pixels.push(g);
        pixels.push(b);
        // tmp.write_u32::<NativeEndian>((0xFF << 24) + (r << 16) + (g << 8) + b)
        //     .unwrap();
    }
    tmp.write_all(pixels.as_slice());
    tmp.flush().unwrap();
}

fn create_shell_surface_event_hander() -> wl_shell_surface::Implementation<()> {
    wl_shell_surface::Implementation {
        ping: |_, _, shell_surface, serial| {
            shell_surface.pong(serial);
        },
        configure: |_, _, _, _, _, _| { /* not used in this example */ },
        popup_done: |_, _, _| { /* not used in this example */ },
    }
}

fn create_callback_event_hander() -> wl_callback::Implementation<ArcRwlAppState> {
    wl_callback::Implementation {
        done: |_, app_state, wl_callback, callback_data| {
            draw(app_state);
        },
    }
}

fn create_pointer_event_hander() -> wl_pointer::Implementation<ArcRwlAppState> {
    wl_pointer::Implementation::<ArcRwlAppState> {
        enter: |_, _, _pointer, _serial, _surface, x, y| {
            println!("Pointer entered surface at ({},{}).", x, y);
        },
        leave: |_, _, _pointer, _serial, _surface| {
            println!("Pointer left surface.");
        },
        motion: |_, app_state, _pointer, _time, x, y| {
            println!("Pointer moved to ({},{}).", x, y);

            // sets Rect's top-left coordinates to that of the pointer
            let mut writable_app_state = app_state.write().unwrap();
            writable_app_state.rect.x = x as u32;
            writable_app_state.rect.y = y as u32;
        },
        button: |_, _, _pointer, _serial, _time, button, state| {
            println!(
                "Button {} ({}) was {:?}.",
                match button {
                    272 => "Left",
                    273 => "Right",
                    274 => "Middle",
                    _ => "Unknown",
                },
                button,
                state
            );
        },
        axis: |_, _, _, _, _, _| {},
        frame: |_, _, _| { /* not used in this example */ },
        axis_source: |_, _, _, _| { /* not used in this example */ },
        axis_discrete: |_, _, _, _, _| { /* not used in this example */ },
        axis_stop: |_, _, _, _, _| { /* not used in this example */ },
    }
}

fn create_keyboard_event_hander() -> wl_keyboard::Implementation<ArcRwlAppState> {
    wl_keyboard::Implementation::<ArcRwlAppState> {
        keymap: |_, _, _keyboard, _serial, _surface, keys| {}, /* not used in this example */
        enter: |_, _, _keyboard, _serial, _surface, keys| {}, /* not used in this example */
        leave: |_, _, _keyboard, _serial, _surface | {},  /* not used in this example */
        key: |_, app_state, _keyboard, _serial, _time, key, state| {
            use wl_keyboard::KeyState;

            let mut writable_app_state = app_state.write().unwrap();

            // update rect coordinates based on keyboard arrow keys
            match (state, key) {
                (KeyState::Released, 103) => {
                    writable_app_state.rect.y = writable_app_state.rect.y - 5;
                },
                (KeyState::Released, 108) => {
                    writable_app_state.rect.y = writable_app_state.rect.y + 5;
                },
                (KeyState::Released, 105) => {
                    writable_app_state.rect.x = writable_app_state.rect.x - 5;
                },
                (KeyState::Released, 106) => {
                    writable_app_state.rect.x = writable_app_state.rect.x + 5;
                }
                _ => ()
            };

            println!("Key {} was {:?}.", key, state);
        },
        modifiers: |_, _, _keyboard, _serial, mods_depressed, mods_latched, mods_locked, group| {},
        repeat_info: |_, _, _keyboard, _serial, _surface| {}
    }
}
