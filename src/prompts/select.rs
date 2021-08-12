use std::iter::FromIterator;

use crate::{
    config::{self, Filter},
    error::{InquireError, InquireResult},
    formatter::{self, OptionFormatter},
    input::Input,
    option_answer::OptionAnswer,
    ui::{crossterm::CrosstermTerminal, Key, KeyModifiers, Renderer, Terminal},
    utils::paginate,
};

/// Prompt suitable for when you need the user to select one option among many.
///
/// The user can select and submit the current highlighted option by pressing space or enter.
///
/// This prompt requires a prompt message and a **non-empty** list of options to be displayed to the user. If the list is empty, the prompt operation will fail with an [`InquireError::InvalidConfiguration`] error.
///
/// This prompt does not support custom validators because of its nature. A submission always selects exactly one of the options. If this option was not supposed to be selected or is invalid in some way, it probably should not be included in the options list.
///
/// The options are paginated in order to provide a smooth experience to the user, with the default page size being 7. The user can move from the options and the pages will be updated accordingly, including moving from the last to the first options (or vice-versa).
///
/// Like all others, this prompt also allows you to customize several aspects of it:
///
/// - **Prompt message**: Required when creating the prompt.
/// - **Options list**: Options displayed to the user. Must be **non-empty**.
/// - **Starting cursor**: Index of the cursor when the prompt is first rendered. Default is 0 (first option). If the index is out-of-range of the option list, the prompt will fail with an [`InquireError::InvalidConfiguration`] error.
/// - **Help message**: Message displayed at the line below the prompt.
/// - **Formatter**: Custom formatter in case you need to pre-process the user input before showing it as the final answer.
///   - Prints the selected option string value by default.
/// - **Page size**: Number of options displayed at once, 7 by default.
/// - **Filter function**: Function that defines if an option is displayed or not based on the current filter input.
///
/// # Example
///
/// ```no_run
/// use inquire::Select;
///
/// let options = vec!["Banana", "Apple", "Strawberry", "Grapes",
///     "Lemon", "Tangerine", "Watermelon", "Orange", "Pear", "Avocado", "Pineapple",
/// ];
///
/// let ans = Select::new("What's your favorite fruit?", &options).prompt();
///
/// match ans {
///     Ok(choice) => println!("{}! That's mine too!", choice.value),
///     Err(_) => println!("There was an error, please try again"),
/// }
/// ```
///
/// [`InquireError::InvalidConfiguration`]: crate::error::InquireError::InvalidConfiguration
#[derive(Copy, Clone)]
pub struct Select<'a> {
    /// Message to be presented to the user.
    pub message: &'a str,

    /// Options displayed to the user.
    pub options: &'a [&'a str],

    /// Help message to be presented to the user.
    pub help_message: Option<&'a str>,

    /// Page size of the options displayed to the user.
    pub page_size: usize,

    /// Whether vim mode is enabled. When enabled, the user can
    /// navigate through the options using hjkl.
    pub vim_mode: bool,

    /// Starting cursor index of the selection.
    pub starting_cursor: usize,

    /// Function called with the current user input to filter the provided
    /// options.
    pub filter: Filter<'a>,

    /// Function that formats the user input and presents it to the user as the final rendering of the prompt.
    pub formatter: OptionFormatter<'a>,
}

impl<'a> Select<'a> {
    /// Default formatter, set to [DEFAULT_OPTION_FORMATTER](crate::formatter::DEFAULT_OPTION_FORMATTER)
    pub const DEFAULT_FORMATTER: OptionFormatter<'a> = formatter::DEFAULT_OPTION_FORMATTER;

    /// Default filter, equal to the global default filter [config::DEFAULT_FILTER].
    pub const DEFAULT_FILTER: Filter<'a> = config::DEFAULT_FILTER;

    /// Default page size.
    pub const DEFAULT_PAGE_SIZE: usize = config::DEFAULT_PAGE_SIZE;

    /// Default value of vim mode.
    pub const DEFAULT_VIM_MODE: bool = config::DEFAULT_VIM_MODE;

    /// Default starting cursor index.
    pub const DEFAULT_STARTING_CURSOR: usize = 0;

    /// Default help message.
    pub const DEFAULT_HELP_MESSAGE: Option<&'a str> =
        Some("↑↓ to move, space or enter to select, type to filter");

    /// Creates a [Select] with the provided message and options, along with default configuration values.
    pub fn new(message: &'a str, options: &'a [&str]) -> Self {
        Self {
            message,
            options,
            help_message: Self::DEFAULT_HELP_MESSAGE,
            page_size: Self::DEFAULT_PAGE_SIZE,
            vim_mode: Self::DEFAULT_VIM_MODE,
            starting_cursor: Self::DEFAULT_STARTING_CURSOR,
            filter: Self::DEFAULT_FILTER,
            formatter: Self::DEFAULT_FORMATTER,
        }
    }

    /// Sets the help message of the prompt.
    pub fn with_help_message(mut self, message: &'a str) -> Self {
        self.help_message = Some(message);
        self
    }

    /// Removes the set help message.
    pub fn without_help_message(mut self) -> Self {
        self.help_message = None;
        self
    }

    /// Sets the page size.
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }

    /// Enables or disabled vim_mode.
    pub fn with_vim_mode(mut self, vim_mode: bool) -> Self {
        self.vim_mode = vim_mode;
        self
    }

    /// Sets the filter function.
    pub fn with_filter(mut self, filter: Filter<'a>) -> Self {
        self.filter = filter;
        self
    }

    /// Sets the formatter.
    pub fn with_formatter(mut self, formatter: OptionFormatter<'a>) -> Self {
        self.formatter = formatter;
        self
    }

    /// Sets the starting cursor index.
    pub fn with_starting_cursor(mut self, starting_cursor: usize) -> Self {
        self.starting_cursor = starting_cursor;
        self
    }

    /// Parses the provided behavioral and rendering options and prompts
    /// the CLI user for input according to the defined rules.
    pub fn prompt(self) -> InquireResult<OptionAnswer> {
        let terminal = CrosstermTerminal::new()?;
        let mut renderer = Renderer::new(terminal)?;
        self.prompt_with_renderer(&mut renderer)
    }

    pub(in crate) fn prompt_with_renderer<T: Terminal>(
        self,
        renderer: &mut Renderer<T>,
    ) -> InquireResult<OptionAnswer> {
        SelectPrompt::new(self)?.prompt(renderer)
    }
}

struct SelectPrompt<'a> {
    message: &'a str,
    options: &'a [&'a str],
    help_message: Option<&'a str>,
    vim_mode: bool,
    cursor_index: usize,
    page_size: usize,
    input: Input,
    filtered_options: Vec<usize>,
    filter: Filter<'a>,
    formatter: OptionFormatter<'a>,
}

impl<'a> SelectPrompt<'a> {
    fn new(so: Select<'a>) -> InquireResult<Self> {
        if so.options.is_empty() {
            return Err(InquireError::InvalidConfiguration(
                "Available options can not be empty".into(),
            ));
        }

        if so.starting_cursor >= so.options.len() {
            return Err(InquireError::InvalidConfiguration(format!(
                "Starting cursor index {} is out-of-bounds for length {} of options",
                so.starting_cursor,
                &so.options.len()
            )));
        }

        Ok(Self {
            message: so.message,
            options: so.options,
            help_message: so.help_message,
            vim_mode: so.vim_mode,
            cursor_index: so.starting_cursor,
            page_size: so.page_size,
            input: Input::new(),
            filtered_options: Vec::from_iter(0..so.options.len()),
            filter: so.filter,
            formatter: so.formatter,
        })
    }

    fn filter_options(&self) -> Vec<usize> {
        self.options
            .iter()
            .enumerate()
            .filter_map(|(i, opt)| match self.input.content() {
                val if val.is_empty() => Some(i),
                val if (self.filter)(&val, opt, i) => Some(i),
                _ => None,
            })
            .collect()
    }

    fn move_cursor_up(&mut self) {
        self.cursor_index = self
            .cursor_index
            .checked_sub(1)
            .or(self.filtered_options.len().checked_sub(1))
            .unwrap_or_else(|| 0);
    }

    fn move_cursor_down(&mut self) {
        self.cursor_index = self.cursor_index.saturating_add(1);
        if self.cursor_index >= self.filtered_options.len() {
            self.cursor_index = 0;
        }
    }

    fn on_change(&mut self, key: Key) {
        match key {
            Key::Up(KeyModifiers::NONE) => self.move_cursor_up(),
            Key::Char('k', KeyModifiers::NONE) if self.vim_mode => self.move_cursor_up(),
            Key::Down(KeyModifiers::NONE) => self.move_cursor_down(),
            Key::Char('j', KeyModifiers::NONE) if self.vim_mode => self.move_cursor_down(),
            key => {
                let dirty = self.input.handle_key(key);

                if dirty {
                    let options = self.filter_options();
                    if options.len() > 0 && options.len() <= self.cursor_index {
                        self.cursor_index = options.len().saturating_sub(1);
                    }
                    self.filtered_options = options;
                }
            }
        };
    }

    fn get_final_answer(&self) -> Option<OptionAnswer> {
        self.filtered_options
            .get(self.cursor_index)
            .and_then(|i| self.options.get(*i).map(|opt| OptionAnswer::new(*i, opt)))
    }

    fn render<T: Terminal>(&mut self, renderer: &mut Renderer<T>) -> InquireResult<()> {
        let prompt = &self.message;

        renderer.reset_prompt()?;

        renderer.print_prompt_input(&prompt, None, &self.input)?;

        let choices = self
            .filtered_options
            .iter()
            .cloned()
            .map(|i| OptionAnswer::new(i, self.options.get(i).unwrap()))
            .collect::<Vec<OptionAnswer>>();

        let page = paginate(self.page_size, &choices, self.cursor_index);

        for (idx, opt) in page.content.iter().enumerate() {
            renderer.print_option(page.selection == idx, &opt.value)?;
        }

        if let Some(help_message) = self.help_message {
            renderer.print_help(help_message)?;
        }

        renderer.flush()?;

        Ok(())
    }

    fn prompt<T: Terminal>(mut self, renderer: &mut Renderer<T>) -> InquireResult<OptionAnswer> {
        let final_answer: OptionAnswer;

        loop {
            self.render(renderer)?;

            let key = renderer.read_key()?;

            match key {
                Key::Cancel => return Err(InquireError::OperationCanceled),
                Key::Submit | Key::Char(' ', KeyModifiers::NONE) => match self.get_final_answer() {
                    Some(answer) => {
                        final_answer = answer;
                        break;
                    }
                    None => {}
                },
                key => self.on_change(key),
            }
        }

        let formatted = (self.formatter)(&final_answer);

        renderer.cleanup(&self.message, &formatted)?;

        Ok(final_answer)
    }
}
