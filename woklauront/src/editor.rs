//! The control panel: key, scale, retune speed, mix.

use nih_plug::prelude::*;
use nih_plug_iced::widgets as nih_widgets;
use nih_plug_iced::*;
use std::sync::Arc;

use crate::WokParams;

pub fn default_state() -> Arc<IcedState> {
    IcedState::from_size(320, 330)
}

pub fn create(params: Arc<WokParams>) -> Option<Box<dyn Editor>> {
    create_iced_editor::<WokEditor>(params.editor_state.clone(), params)
}

pub struct WokEditor {
    params: Arc<WokParams>,
    context: Arc<dyn GuiContext>,

    root_state: nih_widgets::param_slider::State,
    scale_state: nih_widgets::param_slider::State,
    retune_state: nih_widgets::param_slider::State,
    mix_state: nih_widgets::param_slider::State,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// Update a parameter's value.
    ParamUpdate(nih_widgets::ParamMessage),
}

impl IcedEditor for WokEditor {
    type Executor = executor::Default;
    type Message = Message;
    type InitializationFlags = Arc<WokParams>;

    fn new(
        params: Self::InitializationFlags,
        context: Arc<dyn GuiContext>,
    ) -> (Self, Command<Message>) {
        (
            WokEditor {
                params,
                context,
                root_state: Default::default(),
                scale_state: Default::default(),
                retune_state: Default::default(),
                mix_state: Default::default(),
            },
            Command::none(),
        )
    }

    fn context(&self) -> &dyn GuiContext {
        self.context.as_ref()
    }

    fn update(&mut self, _window: &mut WindowQueue, message: Message) -> Command<Message> {
        match message {
            Message::ParamUpdate(message) => self.handle_param_message(message),
        }

        Command::none()
    }

    fn view(&mut self) -> Element<'_, Message> {
        let ink = Color::from_rgb8(48, 44, 40);
        let soft = Color::from_rgb8(110, 110, 125);

        let label = |text: &'static str| {
            Text::new(text)
                .size(10)
                .width(Length::Fill)
                .height(16.into())
                .horizontal_alignment(alignment::Horizontal::Center)
                .vertical_alignment(alignment::Vertical::Center)
                .color(soft)
        };

        Column::new()
            .align_items(Alignment::Center)
            .push(Space::with_height(6.into()))
            .push(Text::new("WOKLAURONT").size(24).color(ink))
            .push(Text::new("hard autotune").size(10).color(soft))
            .push(Space::with_height(10.into()))
            .push(label("root note"))
            .push(
                nih_widgets::ParamSlider::new(&mut self.root_state, &self.params.root)
                    .width(Length::Fill)
                    .map(Message::ParamUpdate),
            )
            .push(Space::with_height(6.into()))
            .push(label("scale"))
            .push(
                nih_widgets::ParamSlider::new(&mut self.scale_state, &self.params.scale)
                    .width(Length::Fill)
                    .map(Message::ParamUpdate),
            )
            .push(Space::with_height(6.into()))
            .push(label("retune speed"))
            .push(
                nih_widgets::ParamSlider::new(&mut self.retune_state, &self.params.retune)
                    .width(Length::Fill)
                    .map(Message::ParamUpdate),
            )
            .push(Space::with_height(6.into()))
            .push(label("mix"))
            .push(
                nih_widgets::ParamSlider::new(&mut self.mix_state, &self.params.mix)
                    .width(Length::Fill)
                    .map(Message::ParamUpdate),
            )
            .push(Space::with_height(8.into()))
            .push(
                Text::new("low retune = robotic. sing on key-ish.")
                    .size(9)
                    .color(soft),
            )
            .into()
    }

    fn background_color(&self) -> nih_plug_iced::Color {
        Color::from_rgba8(255, 244, 214, 1.0)
    }
}
