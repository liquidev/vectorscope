use std::sync::{Arc, Mutex};

use cpal::{
    Stream, StreamConfig, SampleRate, BufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use pixels::{Pixels, SurfaceTexture};
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{WindowBuilder},
};

struct AudioState {
    stream: Stream,
    samples_back: Arc<Mutex<Vec<f32>>>,
    samples_front: Vec<f32>,
    last_sample: (f32, f32),
}

impl AudioState {
    fn new() -> anyhow::Result<Self> {
        // find host
        let host_id = cpal::available_hosts()
            .into_iter()
            .find(|id| *id == cpal::HostId::Jack)
            .expect("could not find JACK host. make sure JACK is installed");
        let host = cpal::host_from_id(host_id)?;
        let input = host.default_input_device().unwrap();

        let arc_samples = Arc::new(Mutex::new(Vec::new()));

        let samples = arc_samples.clone();
        let stream = input.build_input_stream(
            &StreamConfig {
                channels: 2,
                sample_rate: SampleRate(48000),
                buffer_size: BufferSize::Fixed(128),
            },
            move |in_samples: &[f32], _info| {
                let mut samples = samples.lock().unwrap();
                samples.extend(in_samples.iter());
            },
            |error| {
                eprintln!("audio error: {}", error);
            },
        )?;
        stream.play()?;

        Ok(AudioState {
            stream,
            samples_back: arc_samples.clone(),
            samples_front: Vec::new(),
            last_sample: (0.0, 0.0),
        })
    }

    fn get_xy(half_width: f32, half_height: f32, left: f32, right: f32) -> (i32, i32) {
        let x = left * half_width + half_width;
        let y = -right * half_height + half_height;
        (x as i32, y as i32)
    }

    fn render(&mut self, mut plot: Plot) {
        let (half_width, half_height) = (plot.width as f32 / 2.0, plot.height as f32 / 2.0);
        // flip buffers
        {
            const BUFFER_SIZE: usize = 2048;
            let mut samples = self.samples_back.lock().unwrap();
            if samples.len() > BUFFER_SIZE {
                self.samples_front.clear();
                self.samples_front.extend(samples.drain(..));
            }
        }
        // plot the samples
        for channels in self.samples_front.chunks_exact(2) {
            if let [left, right, ..] = channels {
                let (previous_left, previous_right) = self.last_sample;
                let (previous_x, previous_y) = Self::get_xy(half_width, half_height, previous_left, previous_right);
                let (x, y) = Self::get_xy(half_width, half_height, *left, *right);
                plot.dot(previous_x, previous_y, x, y);
                self.last_sample = (*left, *right);
            }
        }
        plot.done();
    }
}

struct Plot<'a, 'b> {
    width: u32,
    height: u32,
    pixels: &'a mut [u8],
    plot: &'b mut [u8],
    previous_pos1: (i32, i32)
}

const fn to_u8(x: i32) -> u8 {
    if x < 0 { 0 }
    else if x > 255 { 255 }
    else { x as u8 }
}

impl Plot<'_, '_> {
    const INTENSITY_COLORS: [(u8, u8, u8); 256] = {
        let mut colors = [(0u8, 0u8, 0u8); 256];
        let mut i = 0;
        while i < 256 {
            let n = i as i32;
            colors[i].1 = to_u8(n * 2);
            let rb = to_u8(((n - 128) as f32 * 1.5) as i32);
            colors[i].0 += rb;
            colors[i].2 += rb;
            i += 1;
        }
        colors
    };

    fn plot_index(&self, x: u32, y: u32) -> usize {
        (x + y * self.width) as usize
    }

    fn pixel_index(&self, x: u32, y: u32) -> usize {
        self.plot_index(x, y) * 4
    }

    fn done(&mut self) {
        const DIVISIONS: u32 = 5;
        const DIV_BRIGHTNESS: u8 = 24;
        let division_width = self.width / DIVISIONS;
        let division_height = self.height / DIVISIONS;
        let half_division_width = division_width / 2;
        let half_division_height = division_height / 2;

        // background
        for pixel in self.pixels.iter_mut() {
            *pixel = 0;
        }
        for div in 0..DIVISIONS {
            let x = half_division_width + division_width * div;
            for y in 0..self.height {
                let i = self.pixel_index(x, y);
                self.pixels[i] = DIV_BRIGHTNESS;
                self.pixels[i + 1] = DIV_BRIGHTNESS;
                self.pixels[i + 2] = DIV_BRIGHTNESS;
            }
            let y = half_division_height + division_height * div;
            for x in 0..self.width {
                let i = self.pixel_index(x, y);
                self.pixels[i] = DIV_BRIGHTNESS;
                self.pixels[i + 1] = DIV_BRIGHTNESS;
                self.pixels[i + 2] = DIV_BRIGHTNESS;
            }
        }

        // dots
        for y in 0..self.height {
            for x in 0..self.width {
                let i = self.pixel_index(x, y);
                let intensity = self.plot[self.plot_index(x, y)];
                let color = Self::INTENSITY_COLORS[intensity as usize];
                self.pixels[i] = self.pixels[i].saturating_add(color.0);
                self.pixels[i + 1] = self.pixels[i + 1].saturating_add(color.1);
                self.pixels[i + 2] = self.pixels[i + 2].saturating_add(color.2);
            }
        }

        // fading out
        for plot in self.plot.iter_mut() {
            *plot = (*plot as f32 * 0.85) as u8;
        }
    }

    fn pixel(&mut self, x: i32, y: i32, intensity: u8) {
        if (0..self.width as i32).contains(&x) && (0..self.height as i32).contains(&y) {
            let (x, y) = (x as u32, y as u32);
            let i = (x + y * self.width) as usize;
            self.plot[i] = self.plot[i].saturating_add(intensity);
        }
    }

    fn point(&mut self, x: i32, y: i32, intensity: u8) {
        self.pixel(x, y, intensity);
//         self.pixel(x + 1, y, intensity);
//         self.pixel(x - 1, y, intensity);
//         self.pixel(x, y + 1, intensity);
//         self.pixel(x, y - 1, intensity);
    }

    fn dot(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (x0, y0) = (x0 as i32, y0 as i32);
        let (x1, y1) = (x1 as i32, y1 as i32);
        for (x, y) in line_drawing::Bresenham::new((x0, y0), (x1, y1)) {
            if (x, y) != self.previous_pos1 {
                self.point(x, y, 16);
            }
        }
        self.previous_pos1 = (x1, y1);
    }
}

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("vectorscope")
        .with_inner_size(LogicalSize::new(800, 800))
        .build(&event_loop)?;

    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
    let mut pixels = Pixels::new(window_size.width, window_size.height, surface_texture)?;

    let mut state = AudioState::new()?;
    let mut plot = Vec::new();
    plot.resize((window_size.width * window_size.height) as usize, 0);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::CloseRequested =>
                        *control_flow = ControlFlow::Exit,
                    WindowEvent::Resized { .. } => {
                        let window_size = window.inner_size();
                        pixels.resize(window_size.width, window_size.height);
                    },
                    _ => (),
                }
            },

            Event::MainEventsCleared => {
                window.request_redraw();
            },

            Event::RedrawRequested(_) => {
                state.render(Plot {
                    width: window_size.width,
                    height: window_size.height,
                    pixels: pixels.get_frame(),
                    plot: &mut plot,
                    previous_pos1: (0, 0),
                });
                match pixels.render() {
                    Err(x) => eprintln!("{}", x),
                    _ => (),
                }
            },

            _ => (),
        }
    })
}
