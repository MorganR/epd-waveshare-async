use bitflags::bitflags;
use core::error::Error as CoreError;
use core::time::Duration;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::{
    prelude::{Dimensions, Point, Size},
    primitives::Rectangle,
};
use embedded_hal::digital::ErrorType as PinErrorType;
use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::{Phase, Polarity},
};
use embedded_hal_async::spi::ErrorType as SpiErrorType;
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};

use crate::{
    buffer::{binary_buffer_length, BinaryBuffer},
    log::{debug, trace},
    Epd, EpdHw,
};

pub trait Epd7in5v2Hw: EpdHw {
    type Power: OutputPin;
    type Error: CoreError
        + From<<Self::Spi as SpiErrorType>::Error>
        + From<<Self::Dc as PinErrorType>::Error>
        + From<<Self::Reset as PinErrorType>::Error>
        + From<<Self::Busy as PinErrorType>::Error>
        + From<<Self::Power as PinErrorType>::Error>;

    fn power(&mut self) -> &mut Self::Power;
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The refresh mode for the display.
pub enum RefreshMode {
    /// Use the full update LUT. This is slower, but should be done occasionally to avoid ghosting.
    ///
    /// It's recommended to avoid full refreshes less than [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] apart,
    /// but to do a full refresh at least every [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Full,
    /// Uses the partial update LUT for fast refresh. A full refresh should be done occasionally to
    /// avoid ghosting, see [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Partial,
    /// This is the standard "fast" update. It uses a different update method, flashing the screen
    /// only once.
    Fast,
}

/// The height of the display (portrait orientation).
pub const DISPLAY_HEIGHT: u16 = 480;
/// The width of the display (portrait orientation).
pub const DISPLAY_WIDTH: u16 = 800;
/// It's recommended to avoid doing a full refresh more often than this (at least on a regular basis).
pub const RECOMMENDED_MIN_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
/// It's recommended to do a full refresh at least this often.
pub const RECOMMENDED_MAX_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const RECOMMENDED_SPI_HZ: u32 = 4_000_000; // 4 MHz
/// Use this phase in conjunction with [RECOMMENDED_SPI_POLARITY] so that the EPD can capture data
/// on the rising edge.
pub const RECOMMENDED_SPI_PHASE: Phase = Phase::CaptureOnFirstTransition;
/// Use this polarity in conjunction with [RECOMMENDED_SPI_PHASE] so that the EPD can capture data
/// on the rising edge.
pub const RECOMMENDED_SPI_POLARITY: Polarity = Polarity::IdleLow;

/// Low-level commands for the Epd7In5v2. You probably want to use the other methods exposed on the
/// [Epd7In5v2] for most operations, but can send commands directly with [Epd7In5v2::send] for low-level
/// control or experimentation.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Set Resolution, LUT selection, BWR pixels, gate scan direction, source shift
    /// direction, booster switch, soft reset.
    PanelSetting = 0x00,

    /// Selecting internal and external power
    PowerSetting = 0x01,

    /// After the Power Off command, the driver will power off following the Power Off
    /// Sequence; BUSY signal will become "0". This command will turn off charge pump,
    /// T-con, source driver, gate driver, VCOM, and temperature sensor, but register
    /// data will be kept until VDD becomes OFF. Source Driver output and Vcom will remain
    /// as previous condition, which may have 2 conditions: 0V or floating.
    PowerOff = 0x02,

    /// Setting Power OFF sequence
    PowerOffSequenceSetting = 0x03,

    /// Turning On the Power
    ///
    /// After the Power ON command, the driver will power on following the Power ON
    /// sequence. Once complete, the BUSY signal will become "1".
    PowerOn = 0x04,

    /// Starting data transmission
    BoosterSoftStart = 0x06,

    /// This command makes the chip enter the deep-sleep mode to save power.
    ///
    /// The deep sleep mode would return to stand-by by hardware reset.
    ///
    /// The only one parameter is a check code, the command would be excuted if check code = 0xA5.
    DeepSleep = 0x07,

    /// This command starts transmitting data and write them into SRAM. To complete data
    /// transmission, command DSP (Data Stop) must be issued. Then the chip will start to
    /// send data/VCOM for panel.
    ///
    /// BLACK/WHITE or OLD_DATA
    DataStartTransmission1 = 0x10,

    /// To stop data transmission, this command must be issued to check the `data_flag`.
    ///
    /// After this command, BUSY signal will become "0" until the display update is
    /// finished.
    DataStop = 0x11,

    /// After this command is issued, driver will refresh display (data/VCOM) according to
    /// SRAM data and LUT.
    ///
    /// After Display Refresh command, BUSY signal will become "0" until the display
    /// update is finished.
    DisplayRefresh = 0x12,

    /// RED or NEW_DATA
    DataStartTransmission2 = 0x13,

    /// Dual SPI - what for?
    DualSpi = 0x15,

    /// The command controls the PLL clock frequency.
    PllControl = 0x30,

    /// This command indicates the interval of Vcom and data output. When setting the
    /// vertical back porch, the total blanking will be kept (20 Hsync).
    VcomAndDataIntervalSetting = 0x50,
    /// This command indicates the input power condition. Host can read this flag to learn
    /// the battery condition.
    LowPowerDetection = 0x51,

    /// This command defines non-overlap period of Gate and Source.
    TconSetting = 0x60,
    /// This command defines alternative resolution and this setting is of higher priority
    /// than the RES\[1:0\] in R00H (PSR).
    TconResolution = 0x61,
    /// This command defines MCU host direct access external memory mode.
    SpiFlashControl = 0x65,

    /// The LUT_REV / Chip Revision is read from OTP address = 25001 and 25000.
    Revision = 0x70,
    /// This command reads the IC status.
    GetStatus = 0x71,

    /// This command implements related VCOM sensing setting.
    AutoMeasurementVcom = 0x80,
    /// This command gets the VCOM value.
    ReadVcomValue = 0x81,
    /// This command sets `VCOM_DC` value.
    VcmDcSetting = 0x82,

    /// This command sets the window size for partial display updates
    SetPartialWindow = 0x90,
    /// Display enters partial mode
    EnterPartialMode = 0x91,
    /// Display exits partial mode, this is never used by the example code. \
    /// Not sure if this command is necessary
    ExitPartialMode = 0x92,

    /// Settings for cascade, setting D1 to 1 allows  TS_SET[7:0] to control the temperature value
    /// This can be overridden with the ForceTemperature command
    CascadeSetting = 0xe0,
    /// This command is used for cascade to fix the temperature value of master and slave chip
    /// Sets TS_SET[7:0]
    ForceTemperature = 0xe5,
}

impl Command {
    /// Returns the register address for this command.
    fn register(&self) -> u8 {
        *self as u8
    }
}

bitflags! {
    pub struct DataFlags: u8 {
        const EnableBorderHiZ = 0b1000_0000;
        // These names make sense for data polarity where 0 means off/white
        const BorderWhite = 0b0001_0000;
        const BorderBlack = 0b0010_0000;

        const NewToOldCopy = 0b0000_1000;

        //Positive polarity: 0 = white
        const PosPol    = 0b0000_0000;
        //Positive polarity: 0 = black
        const NegPol    = 0b0000_0001;
        // Disables the usage of different LUTs dependent on the difference between the old
        // and new framebuffer
        const DisableNO = 0b0000_0010;

    }
}

const VCOM_INTERVAL_10: u8 = 0x07;

/// The buffer type used by [Epd7In5v2].
pub type Epd7In5v2Buffer =
    BinaryBuffer<{ binary_buffer_length(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32)) }>;

pub struct Epd7In5v2<HW>
where
    HW: EpdHw,
{
    hw: HW,
    refresh_mode: Option<RefreshMode>,
    data_settings: DataFlags,
}

impl<HW> Epd7In5v2<HW>
where
    HW: Epd7in5v2Hw,
{
    pub fn new(hw: HW) -> Self {
        Epd7In5v2 {
            hw,
            refresh_mode: None,
            data_settings: DataFlags::BorderWhite,
        }
    }

    /// Sets the power pin high for the EPD. Before you can start using the display you must
    /// call [Epd::init] to properly initialize the display.
    pub async fn turn_on(&mut self) -> Result<(), <HW as Epd7in5v2Hw>::Error> {
        self.hw.power().set_high()?;
        Ok(())
    }

    /// Sets the power pin low for the EPD. This is the end of the shutdown cycle of the display,
    /// which starts by calling [Epd::sleep].
    pub async fn turn_off(&mut self) -> Result<(), <HW as Epd7in5v2Hw>::Error> {
        self.hw.power().set_low()?;
        Ok(())
    }

    /// Sets the border to the specified colour. You need to call [Epd::update_display] using
    /// [RefreshMode::Full] afterward to apply this change.
    ///
    /// Note: on my board, the white setting fades to grey fairly quickly. I have not found a way
    /// to avoid this.
    pub async fn set_border(
        &mut self,
        spi: &mut HW::Spi,
        color: BinaryColor,
    ) -> Result<(), <HW as EpdHw>::Error> {
        match color {
            BinaryColor::Off => {
                self.data_settings &= !DataFlags::BorderBlack;
                self.data_settings |= DataFlags::BorderWhite;
            }
            BinaryColor::On => {
                self.data_settings &= !DataFlags::BorderWhite;
                self.data_settings |= DataFlags::BorderBlack;
            }
        };
        self.send(
            spi,
            Command::VcomAndDataIntervalSetting,
            &[self.data_settings.bits(), VCOM_INTERVAL_10],
        )
        .await
    }

    /// Writes buffer data into the old internal framebuffer. This can be useful either:
    ///
    /// * to prep the next frame before the current one has been displayed (since the old buffer
    ///   becomes the current buffer after the next call to [Self::update_display()]).
    /// * to modify the "diff" that is displayed if in [RefreshMode::Partial]. Also see [Command::DisplayUpdateControl1].
    pub async fn write_old_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Epd7In5v2Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.send(spi, Command::DataStartTransmission1, buffer.data())
            .await
    }

    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    pub async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), <HW as EpdHw>::Error> {
        trace!("Sending EPD command: {:?}", command);

        // Low for command
        self.hw.dc().set_low()?;
        spi.write(&[command.register()]).await?;

        if !data.is_empty() {
            // High for data
            self.hw.dc().set_high()?;
            spi.write(data).await?;
        }

        Ok(())
    }

    /// Waits for the current operation to complete if the display is busy.
    ///
    /// Note that this will wait forever if the display is asleep.
    async fn wait_if_busy(&mut self, _spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        // Note: the datasheet states that busy pin is active low, i.e. we should wait for it when
        // it's low.
        if self.hw.busy().is_low()? {
            trace!("Waiting for busy EPD");
            self.hw.busy().wait_for_low().await?;
        }
        Ok(())
    }

    async fn init_part(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Initialising display for partial updates");
        self.reset().await?;

        self.wait_if_busy(spi).await?;
        self.send(spi, Command::PanelSetting, &[0x1f]).await?;
        self.send(spi, Command::PowerOn, &[]).await?;
        self.hw.delay().delay_ms(100).await;
        self.wait_if_busy(spi).await?;

        self.send(spi, Command::CascadeSetting, &[0x02]).await?;
        self.send(spi, Command::ForceTemperature, &[0x6e]).await?;

        Ok(())
    }

    async fn init_fast(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Initialising display for partial updates");
        self.reset().await?;

        self.wait_if_busy(spi).await?;
        self.send(spi, Command::PanelSetting, &[0x1f]).await?;
        self.data_settings = DataFlags::BorderWhite;
        self.send(
            spi,
            Command::VcomAndDataIntervalSetting,
            &[self.data_settings.bits(), VCOM_INTERVAL_10],
        )
        .await?;
        self.send(spi, Command::PowerOn, &[]).await?;
        self.hw.delay().delay_ms(100).await;
        self.wait_if_busy(spi).await?;

        self.send(spi, Command::BoosterSoftStart, &[0x27, 0x27, 0x18, 0x17])
            .await?;

        self.send(spi, Command::CascadeSetting, &[0x02]).await?;
        self.send(spi, Command::ForceTemperature, &[0x5a]).await?;

        Ok(())
    }
}

impl<HW> Epd<HW> for Epd7In5v2<HW>
where
    HW: Epd7in5v2Hw,
{
    type RefreshMode = RefreshMode;
    type Buffer = Epd7In5v2Buffer;

    fn new_buffer(&self) -> Self::Buffer {
        BinaryBuffer::new(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32))
    }

    fn width(&self) -> u32 {
        DISPLAY_WIDTH as u32
    }

    fn height(&self) -> u32 {
        DISPLAY_HEIGHT as u32
    }
    async fn init(
        &mut self,
        spi: &mut HW::Spi,
        mode: RefreshMode,
    ) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Initialising display");
        self.reset().await?;

        self.wait_if_busy(spi).await?;
        // Reset all configurations to default.
        self.send(spi, Command::BoosterSoftStart, &[0x17, 0x17, 0x28, 0x17])
            .await?;
        self.send(spi, Command::PowerSetting, &[0x07, 0x07, 0x3F, 0x3F])
            .await?;
        self.send(spi, Command::PowerOn, &[]).await?;
        self.hw.delay().delay_ms(100).await;
        self.wait_if_busy(spi).await?;
        self.send(spi, Command::PanelSetting, &[0x1f]).await?;
        self.send(spi, Command::PllControl, &[0x06]).await?;
        self.send(spi, Command::TconResolution, &[0x03, 0x20, 0x01, 0xe0])
            .await?;
        self.send(spi, Command::DualSpi, &[0x00]).await?;
        self.data_settings = DataFlags::BorderWhite;
        self.send(
            spi,
            Command::VcomAndDataIntervalSetting,
            &[self.data_settings.bits(), VCOM_INTERVAL_10],
        )
        .await?;
        self.send(spi, Command::TconSetting, &[0x22]).await?;
        self.wait_if_busy(spi).await?;

        match mode {
            RefreshMode::Partial => self.init_part(spi).await?,
            RefreshMode::Fast => self.init_fast(spi).await?,
            _ => {
                // Other modes need no special initialization
            }
        }
        Ok(())
    }

    async fn set_refresh_mode(
        &mut self,
        spi: &mut <HW as EpdHw>::Spi,
        mode: Self::RefreshMode,
    ) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Changing refresh mode to {:?}", mode);
        self.refresh_mode = Some(mode);

        // Update bypass if needed.
        match mode {
            RefreshMode::Full => self.init(spi, mode).await,
            RefreshMode::Partial => self.init_part(spi).await,
            RefreshMode::Fast => self.init_fast(spi).await,
        }
    }

    async fn reset(&mut self) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Resetting EPD");
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(10).await;
        self.hw.reset().set_low()?;
        self.hw.delay().delay_ms(2).await;
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(200).await;
        Ok(())
    }

    async fn sleep(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Sleeping EPD");
        self.wait_if_busy(spi).await?;
        self.send(spi, Command::PowerOff, &[]).await?;
        self.wait_if_busy(spi).await?;
        self.send(spi, Command::DeepSleep, &[0xa5]).await?;
        Ok(())
    }

    async fn wake(&mut self, _spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Waking EPD");
        self.reset().await
        // Confirmed with a physical screen that init is not required after waking.
    }

    async fn display_buffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.wait_if_busy(spi).await?;
        self.write_old_framebuffer(spi, &buffer).await?;
        self.write_framebuffer(spi, buffer).await?;
        self.update_display(spi).await?;
        self.wait_if_busy(spi).await?;
        Ok(())
    }

    async fn display_partial_buffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
        area: Rectangle,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.wait_if_busy(spi).await?;

        self.data_settings = DataFlags::EnableBorderHiZ
            | DataFlags::BorderBlack
            | DataFlags::NewToOldCopy
            | DataFlags::PosPol;
        self.send(
            spi,
            Command::VcomAndDataIntervalSetting,
            &[self.data_settings.bits(), VCOM_INTERVAL_10],
        )
        .await?;
        //Enter partial mode
        self.send(spi, Command::EnterPartialMode, &[]).await?;
        // If the area is of size zero, it is a point. The bottom right == upper left.
        let bottom_right = area
            .bottom_right()
            .unwrap_or(Point::new(area.top_left.x, area.top_left.y));

        let min_x = round_down_8_multiple(area.top_left.x as u16);
        let max_x = round_up_8_multiple(area.bottom_right().unwrap().x as u16);
        // let max_x = (bottom_right.x / 8 * 8 + 1) as u16;
        let row_length = max_x - min_x;
        let row_num_bytes = row_length / 8;

        let min_y = area.top_left.y as u16;
        let max_y = bottom_right.y as u16;

        let min_x_bytes = min_x.to_be_bytes();
        let max_x_bytes = max_x.to_be_bytes();
        let min_y_bytes = min_y.to_be_bytes();
        let max_y_bytes = max_y.to_be_bytes();

        self.send(
            spi,
            Command::SetPartialWindow,
            &[
                min_x_bytes[0],
                min_x_bytes[1],
                max_x_bytes[0],
                max_x_bytes[1],
                min_y_bytes[0],
                min_y_bytes[1],
                max_y_bytes[0],
                max_y_bytes[1],
                0x01,
            ],
        )
        .await?;

        // Low for command
        self.hw.dc().set_low()?;
        spi.write(&[Command::DataStartTransmission2.register()])
            .await?;

        let full_data = buffer.data();

        // High for data
        self.hw.dc().set_high()?;
        for j in min_y..=max_y {
            let start_index =
                ((j as u32 * buffer.bounding_box().size.width + min_x as u32) / 8) as usize;
            let stop_index = start_index + row_num_bytes as usize;
            spi.write(&full_data[start_index..=stop_index]).await?;
            trace!("Wrote: {:?}", &full_data[start_index..=stop_index]);
        }

        self.update_display(spi).await?;
        self.wait_if_busy(spi).await?;
        // Exit partial mode
        self.send(spi, Command::ExitPartialMode, &[]).await?;
        Ok(())
    }

    async fn write_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.wait_if_busy(spi).await?;
        self.write_image(spi, buffer.data()).await?;
        Ok(())
    }

    async fn set_window(
        &mut self,
        _spi: &mut HW::Spi,
        _shape: Rectangle,
    ) -> Result<(), <HW as EpdHw>::Error> {
        unimplemented!("Setting window is not needed for this EPD type");
    }

    async fn set_cursor(
        &mut self,
        _spi: &mut HW::Spi,
        _position: Point,
    ) -> Result<(), <HW as EpdHw>::Error> {
        unimplemented!("The display hardware has no 'cursor'");
    }

    async fn write_image(
        &mut self,
        spi: &mut HW::Spi,
        image: &[u8],
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.wait_if_busy(spi).await?;
        self.send(spi, Command::DataStartTransmission2, image)
            .await?;
        Ok(())
    }

    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        // Enable the clock and CP (?), and then display the data from the RAM. Note that there are
        // two RAM buffers, so this will swap the active buffer. Calling this function twice in a row
        // without writing further to RAM therefore results in displaying the previous image.
        debug!("Updating display");
        self.send(spi, Command::DisplayRefresh, &[]).await?;
        self.hw.delay().delay_ms(100).await;
        self.wait_if_busy(spi).await?;
        Ok(())
    }
}

#[inline(always)]
fn round_down_8_multiple(x: u16) -> u16 {
    x / 8 * 8
}

#[inline(always)]
fn round_up_8_multiple(x: u16) -> u16 {
    (x + 7) & !7
}
