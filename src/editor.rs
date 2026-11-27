//! The control panel: one knob. That's all you get.

use nih_plug::prelude::*;
use nih_plug_iced::widgets as nih_widgets;
use nih_plug_iced::*;
use std::sync::Arc;

use crate::HogSpasmParams;

pub fn default_state() -> Arc<IcedState> {
    IcedState::from_size(300, 210)
}

pub fn create(params: Arc<HogSpasmParams>) -> Option<Box<dyn Editor>> {
    create_iced_editor::<HogEditor>(params.editor_state.clone(), params)
}

pub struct HogEditor {
    params: Arc<HogSpasmParams>,
    context: Arc<dyn GuiContext>,

    intensity_state: nih_widgets::param_slider::State,
    oinkiness_state: nih_widgets::param_slider::State,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// Update a parameter's value.
    ParamUpdate(nih_widgets::ParamMessage),
}

impl IcedEditor for HogEditor {
    type Executor = executor::Default;
    type Message = Message;
    type InitializationFlags = Arc<HogSpasmParams>;

    fn new(
        params: Self::InitializationFlags,
        context: Arc<dyn GuiContext>,
    ) -> (Self, Command<Message>) {
        (
            HogEditor {
                params,
                context,
                intensity_state: Default::default(),
                oinkiness_state: Default::default(),
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
        let soft = Color::from_rgb8(150, 95, 110);

        Column::new()
            .align_items(Alignment::Center)
            .push(Space::with_height(8.into()))
            .push(Text::new("HOGSPASM").size(24).color(ink))
            .push(
                Text::new("the herd summons the pig pics")
                    .size(10)
                    .color(soft),
            )
            .push(Space::with_height(10.into()))
            .push(
                nih_widgets::ParamSlider::new(&mut self.intensity_state, &self.params.intensity)
                    .width(Length::Fill)
                    .map(Message::ParamUpdate),
            )
            .push(Space::with_height(6.into()))
            .push(
                Text::new("hog intensity")
                    .size(10)
                    .color(soft),
            )
            .push(Space::with_height(10.into()))
            .push(
                nih_widgets::ParamSlider::new(&mut self.oinkiness_state, &self.params.oinkiness)
                    .width(Length::Fill)
                    .map(Message::ParamUpdate),
            )
            .push(Space::with_height(6.into()))
            .push(
                Text::new("oinkiness")
                    .size(10)
                    .color(soft),
            )
            .push(Space::with_height(6.into()))
            .push(
                Text::new("psst... type piggy anywhere to calm the herd")
                    .size(9)
                    .color(soft),
            )
            .into()
    }

    fn background_color(&self) -> nih_plug_iced::Color {
        Color::from_rgba8(255, 232, 239, 1.0)
    }
}
