#![no_main]
#![no_std]

extern crate panic_semihosting;
extern crate stm32f1xx_hal as hal;

use cortex_m::singleton;
use rtic::app;
use stm32f1xx_hal::prelude::*;
use rtic::cyccnt::{Instant, U32Ext as _};
use cortex_m_semihosting::hprintln;
use hal::{
    dma::{dma1::C3, TxDma},
    gpio::{gpioa, gpioa::*},
    spi::{Mode, Phase, Polarity, Spi, SpiPayload},
    time::{MegaHertz},
};

use smart_leds::RGB8;
use ws2812_spi_dma as ws2812;
use ws2812::spi_bit_container;

const LED_COUNT: usize = 50;
const SYS_CLK: MegaHertz = MegaHertz(48);
const PCLK1: MegaHertz = MegaHertz(24);
spi_bit_container!(LedBitContainer, LED_COUNT);

type SpiDma = TxDma<
    SpiPayload<
        hal::pac::SPI1,
        hal::spi::Spi1NoRemap,
        (
            gpioa::PA5<hal::gpio::Alternate<hal::gpio::PushPull>>,
            gpioa::PA6<hal::gpio::Input<hal::gpio::Floating>>,
            PA7<hal::gpio::Alternate<hal::gpio::PushPull>>,
        ),
    >,
    C3,
>;
type TransferResult = (&'static mut LedBitContainer, SpiDma);

#[app(device = stm32f1xx_hal::pac, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        spi_dma: Option<SpiDma>,
        led_buffer: Option<&'static mut LedBitContainer>,
        pixels: Option<[RGB8; LED_COUNT]>,
        #[init(0)]
        current_hue: usize,
    }

    #[init(schedule = [exe])]
    fn init(mut cx: init::Context) -> init::LateResources {
        let mut core = cx.core;
        // Initialize (enable) the monotonic timer (CYCCNT)
        core.DWT.enable_cycle_counter();
        hprintln!("init @ {:?}", cx.start).unwrap();

        // Cortex-M peripherals
        let mut rcc = cx.device.RCC.constrain();
        let mut flash = cx.device.FLASH.constrain();
        let mut mapr = cx.device.AFIO.constrain(&mut rcc.apb2).mapr;
        let clocks = rcc
            .cfgr
            .sysclk(SYS_CLK)
            .pclk1(PCLK1)
            .freeze(&mut flash.acr);

        hprintln!("Initialising Ws2812 LEDs").unwrap();

        // Set up pins for SPI and create SPI interface
        let mut gpioa = cx.device.GPIOA.split(&mut rcc.apb2);
        let pins = (
            gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl),
            gpioa.pa6.into_floating_input(&mut gpioa.crl),
            gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl),
        );
        let spi_mode = Mode {
            polarity: Polarity::IdleLow,
            phase: Phase::CaptureOnFirstTransition,
        };
        let mut spi = Spi::spi1(
            cx.device.SPI1,
            pins,
            &mut mapr,
            spi_mode,
            3.mhz(),
            clocks,
            &mut rcc.apb2,
        );

        let dma = cx.device.DMA1.split(&mut rcc.ahb);
        let spi_dma: SpiDma = spi.with_tx_dma(dma.3);

        cx.schedule.exe(cx.start).unwrap();
        let led_buffer = singleton!(: LedBitContainer = LedBitContainer::new());
        

        let leds: &'static mut LedBitContainer = led_buffer.unwrap();
        let pixels = [RGB8::new(0,0,0); LED_COUNT];
        led_spi_bit_pattern(&pixels, &mut leds.data);

        
        let transfer = spi_dma.write(leds);
        let result: TransferResult = transfer.wait();
        
        init::LateResources {
            spi_dma: Some(result.1),
            led_buffer: singleton!(: LedBitContainer = LedBitContainer::new()),
            pixels: Some(pixels),
        }
    }

    #[task(schedule = [exe], resources = [spi_dma, led_buffer, pixels, current_hue])]
    fn exe(cx: exe::Context) {

        let leds = cx.resources.led_buffer.take().unwrap(); 
        let spi_dma: SpiDma = cx.resources.spi_dma.take().unwrap();
        let mut color = cx.resources.pixels.take().unwrap();

        let current_hue = *cx.resources.current_hue;
        for i in 0..LED_COUNT {
            color[i] = wheel((current_hue + i) as u8 & 255);
        }
        //let _ = hprintln!("{}", color[0]);
        led_spi_bit_pattern(&color, &mut leds.data);
        let tx = spi_dma.write(leds);
        let result: TransferResult = tx.wait();        

        *cx.resources.led_buffer = Some(result.0);
        *cx.resources.spi_dma = Some(result.1);
        *cx.resources.pixels = Some(color);
        *cx.resources.current_hue = current_hue + 1;

        cx.schedule.exe(cx.scheduled + 100_000.cycles()).unwrap();
    }

    extern "C" {
        fn EXTI0();
    }
};

pub fn led_spi_bit_pattern(
    leds: &[RGB8],
    mut output: &mut [u8]
) {
    // Set all LEDS to 0
    for i in 0..output.len() {
        output[i] = 0;
    }
    for led in leds {
        set_from_byte(led.g, output);
        output = &mut output[8..];
        set_from_byte(led.r, output);
        output = &mut output[8..];
        set_from_byte(led.b, output);
        output = &mut output[8..];
    }
}

fn set_from_byte(byte: u8, mut target: &mut [u8]) {
    for i in 0..8 {
        const MASK: u8 = 0b1000_0000;
        set_spi_byte((byte << i) & MASK == MASK, target);
        target = &mut target[1..]
    }
}

fn set_spi_byte(value: bool, target: &mut [u8]) {
    target[0] = match value {
        false => 0b10000000,
        true => 0b11110000,
    };
}



/// Input a value 0 to 255 to get a color value
/// The colours are a transition r - g - b - back to r.
fn wheel(mut wheel_pos: u8) -> RGB8 {
    wheel_pos = 255 - wheel_pos;
    if wheel_pos < 85 {
        return (255 - wheel_pos * 3, 0, wheel_pos * 3).into();
    }
    if wheel_pos < 170 {
        wheel_pos -= 85;
        return (0, wheel_pos * 3, 255 - wheel_pos * 3).into();
    }
    wheel_pos -= 170;
    (wheel_pos * 3, 255 - wheel_pos * 3, 0).into()
}


mod test {
    fn test() {

    }
}
