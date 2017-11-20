#[macro_use]
extern crate wayland_client;

extern crate tempfile;

extern crate byteorder;
use byteorder::{NativeEndian, WriteBytesExt};
use std::cmp::min;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, RwLock};
use wayland_client::EnvHandler;
use wayland_client::protocol::{wl_compositor, wl_pointer, wl_seat, wl_shell, wl_shell_surface,
                               wl_shm};

// buffer (and window) width and height
const BUF_X: u32 = 320;
const BUF_Y: u32 = 240;

wayland_env!(
    WaylandEnvironment,
    compositor: wl_compositor::WlCompositor,
    seat: wl_seat::WlSeat,
    shell: wl_shell::WlShell,
    shm: wl_shm::WlShm
);

struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

struct AppState {
    rect: Rect,
}

type ArcRwlAppState = Arc<RwLock<AppState>>;

impl AppState {
    fn new() -> ArcRwlAppState {
        Arc::new(RwLock::new(AppState {
            rect: Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 50,
            },
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

    // match a buffer on the part we wrote on
    let buffer = pool.create_buffer(
        0,
        BUF_X as i32,
        BUF_Y as i32,
        (BUF_X * 4) as i32,
        wl_shm::Format::Argb8888,
    ).expect("The pool cannot be already dead");

    // make our surface as a toplevel one
    shell_surface.set_toplevel();
    // attach the buffer to it
    surface.attach(Some(&buffer), 0, 0);
    // commit
    surface.commit();

    let pointer = env.seat
        .get_pointer()
        .expect("Seat cannot be already destroyed.");

    let app_state = AppState::new();

    event_queue.register(&shell_surface, shell_surface_impl(), ());
    event_queue.register(&pointer, pointer_impl(), app_state.clone());


    loop {
        draw(&app_state, &mut tmp);
        display.flush().expect("Error flushing display");
        event_queue.dispatch().expect("Event queue dispatch failed");
    }
}

fn draw(app_state: &ArcRwlAppState, tmp: &mut File) {
    use std::io::{Seek, SeekFrom};

    let readable_app_state = app_state.read().unwrap();

    println!(
        "Rect moved to ({},{}).",
        readable_app_state.rect.x,
        readable_app_state.rect.y
    );

    tmp.seek(SeekFrom::Start(0)).unwrap();

    fn is_coords_in_rect(rect: &Rect, i: u32, j: u32) -> bool {
        i > rect.x && i < rect.x + rect.w && j > rect.y && j < rect.y + rect.h
    }

    // write the contents to it, lets put a nice color gradient
    for i in 0..(BUF_X * BUF_Y) {
        let x = (i % BUF_X) as u32;
        let y = (i / BUF_Y) as u32;

        let mut r = 0u32;
        let mut g = 0u32;
        let mut b = 0u32;

        if is_coords_in_rect(&readable_app_state.rect, x, y) {
            r = 255;
            g = 255;
            b = 255;
        }
        tmp.write_u32::<NativeEndian>((0xFF << 24) + (r << 16) + (g << 8) + b)
            .unwrap();
    }
    tmp.flush().unwrap();
}

fn shell_surface_impl() -> wl_shell_surface::Implementation<()> {
    wl_shell_surface::Implementation {
        ping: |_, _, shell_surface, serial| {
            shell_surface.pong(serial);
        },
        configure: |_, _, _, _, _, _| { /* not used in this example */ },
        popup_done: |_, _, _| { /* not used in this example */ },
    }
}

fn pointer_impl() -> wl_pointer::Implementation<ArcRwlAppState> {
    wl_pointer::Implementation::<ArcRwlAppState> {
        enter: |_, _, _pointer, _serial, _surface, x, y| {
            println!("Pointer entered surface at ({},{}).", x, y);
        },
        leave: |_, _, _pointer, _serial, _surface| {
            println!("Pointer left surface.");
        },
        motion: |_, app_state, _pointer, _time, x, y| {
            println!("Pointer moved to ({},{}).", x, y);

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
        axis: |_, _, _, _, _, _| { /* not used in this example */ },
        frame: |_, _, _| { /* not used in this example */ },
        axis_source: |_, _, _, _| { /* not used in this example */ },
        axis_discrete: |_, _, _, _, _| { /* not used in this example */ },
        axis_stop: |_, _, _, _, _| { /* not used in this example */ },
    }
}
