#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::convert::Infallible;
use embedded_nrf24l01 as nrf;
use hal::{
    gpio::{gpiob::*, Alternate, Floating, Input, Output, PushPull, PullUp},
    prelude::*,
};
use rtic::cyccnt::{U32Ext as _};
use rtic::app;
use stm32f1::stm32f103::SPI2;
use stm32f1xx_hal as hal;
use cortex_m_semihosting::hprintln;
use hal::{
    spi::{Spi, Spi2NoRemap},
    time::MegaHertz,
};

use nrf::{Configuration, CrcMode, DataRate, StandbyMode, NRF24L01, RxMode};

type RadioCe = PB0<Output<PushPull>>;
type RadioCsn = PB1<Output<PushPull>>;
type RadioIrq = PB10<Input<PullUp>>;
type RadioSpi2Pins = (
    PB13<Alternate<PushPull>>,
    PB14<Input<Floating>>,
    PB15<Alternate<PushPull>>,
);
type RadioSpi = Spi<SPI2, Spi2NoRemap, RadioSpi2Pins>;
type Radio = NRF24L01<Infallible, RadioCe, RadioCsn, RadioSpi>;

// From https://github.com/davidji/rust-rc
pub const TX_ADDRESS: [u8; 5] = ['R' as u8, 'C' as u8, 'T' as u8, 'X' as u8, 0x00];
pub const RX_ADDRESS: [u8; 5] = ['R' as u8, 'C' as u8, 'R' as u8, 'X' as u8, 0x00];
pub const BUFFER_SIZE: usize = 32;

const FREQ: u32 = 48;
const SYSCLK_FREQ: MegaHertz = MegaHertz(FREQ);
const PCLK1_FREQ: MegaHertz = MegaHertz(FREQ / 2);

#[app(device = stm32f1xx_hal::pac, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        radio: Option<StandbyMode<Radio>>,
        buffer: Option<[u8; BUFFER_SIZE]>,
        // irq: Option<RadioIrq>,
    }

    #[init]
    fn init(cx: init::Context) -> init::LateResources {
        hprintln!("Initializing device!").unwrap();
        // Enable the monotonic timer
        let mut core = cx.core;
        core.DWT.enable_cycle_counter();
        // let _ = cx.start;

        hprintln!("Setting up peripherals!").unwrap();
        let mut rcc = cx.device.RCC.constrain();
        let mut flash = cx.device.FLASH.constrain();
        let _mapr = cx.device.AFIO.constrain(&mut rcc.apb2).mapr;
        let clocks = rcc
            .cfgr
            .sysclk(SYSCLK_FREQ)
            .pclk1(PCLK1_FREQ)
            .freeze(&mut flash.acr);

        hprintln!("Setting up SPI!").unwrap();
        let mut gpiob = cx.device.GPIOB.split(&mut rcc.apb2);

        let spi_pins: RadioSpi2Pins = (
            gpiob.pb13.into_alternate_push_pull(&mut gpiob.crh),
            gpiob.pb14.into_floating_input(&mut gpiob.crh),
            gpiob.pb15.into_alternate_push_pull(&mut gpiob.crh),
        );

        // Pins for the wireless transmitter
        let (ce, csn): (RadioCe, RadioCsn) = (
            gpiob.pb0.into_push_pull_output(&mut gpiob.crl),
            gpiob.pb1.into_push_pull_output(&mut gpiob.crl),
        );

        let spi: RadioSpi = Spi::spi2(
            cx.device.SPI2,
            spi_pins,
            nrf::setup::spi_mode(),
            nrf::setup::clock_mhz().mhz(),
            clocks,
            &mut rcc.apb1,
        );

        hprintln!("Setting up the radio!").unwrap();
        
        // Set up interrupt
        // let mut irq = gpiob.pb10.into_pull_up_input();
        // irq.make_interrupt_source(&mut peripherals.SYSCFG);
        // irq.trigger_on_edge(&mut peripherals.EXTI, Edge::FALLING);
        // irq.enable_interrupt(&mut peripherals.EXTI);

        // Create radio
        let mut radio: StandbyMode<Radio> = NRF24L01::new(ce, csn, spi).expect("to create a new radio interface");

        radio.set_frequency(FREQ as u8).expect("to set frequency");
        radio.set_rx_addr(0, &RX_ADDRESS).expect("to set address");
        radio.set_tx_addr(&RX_ADDRESS).expect("to set tx address");
        radio.set_rf(DataRate::R250Kbps, 0).expect("to set frequency");
        radio.set_auto_retransmit(0b0100, 15).expect("to set retransmit");
        radio.set_auto_ack(&[true; 6]).unwrap();
        radio.set_crc(Some(CrcMode::TwoBytes)).expect("to set crc mode");
        radio.set_pipes_rx_lengths(&[None; 6]).expect("to set pipes length");
        radio.set_pipes_rx_enable(&[true, false, false, false, false, false]).unwrap();
        radio.flush_tx().expect("to flush tx");
        radio.flush_rx().expect("to flush rx");
        
        radio.set_interrupt_mask(true, true, true).unwrap();
        radio.set_interrupt_mask(false, false, false).unwrap();
        radio.clear_interrupts().unwrap();

        hprintln!("Beginning to receive transmissions!").unwrap();
        
        init::LateResources {
            radio: Some(radio),
            buffer: Some([0u8; 32]),
        }
    }

    #[idle(resources = [radio])]
    fn idle(cx: idle::Context) -> ! {
        let nrf = cx.resources.radio.take().expect("Radio is not available");
        let rx = &mut nrf.rx().expect("Radio could not be set to receive mode");

        loop {
            let pipe = rx.can_read().unwrap();

            if pipe.is_some() {
                let data = rx.read().unwrap();
                hprintln!("{:?}", data.as_ref());
                // cx.resources.LED.toggle().unwrap();
            }
        }
    }

    extern "C" {
        fn EXTI0();
    }
};
