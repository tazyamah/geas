# How geas thinks about dates and times

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

The basic stance of geas is to **think of real datetimes and virtual datetimes separately**. Nothing beyond that is decided.

- A **real datetime** is very close to a physical type. Unless specified otherwise, it follows the convenience of the target language (for Rust, for now, an integer number of seconds; if time zones or calendars are needed for display or reading, a standard crate such as chrono). What the language decides is only that times and durations (`sec min hour`) can be added, subtracted and compared.
- A **virtual datetime** means different things to different people and applications using geas, so the language does not decide it. Only the stance of thinking of them separately is fixed.

The implementation goes as far as addition and subtraction of real datetimes.

## Two kinds

| Kind | Meaning | Representation | Now in geas |
|---|---|---|---|
| Real datetime | A point on the time axis | Close to a physical type; follows the target's convenience (Rust: for now an integer number of seconds). Almost no logical layer | `time`. Durations `sec min hour` can be added and subtracted; the difference of two times is a number of seconds |
| Virtual datetime | Something like "this year's birthday", where hours, minutes and seconds are unspecified, so it is not a point as it stands | A group of calendar components (year, month, day, …) | Not implemented |

A virtual datetime is converted to a real datetime only when needed, by supplying the missing parts (hours, minutes, seconds, calendar, time zone). If it is not converted, it does not exist as a real moment.

## What "one day later" means

What people call "one day after this date" is a calculation on a virtual datetime (a date), while "86,400 seconds later" is a calculation on a real datetime; they are different things. With daylight saving time and so on, a calendar day is not always 86,400 seconds.

- `day`, `week`, `month` and `year` cannot be written as durations of a real datetime (writing them is an error). An exact length of time is written `24 hour`.
- Calendar units are attached to the virtual-datetime side.
- A time zone is needed only for converting between a virtual and a real datetime (in either direction), not for calendar calculation itself.

## The built-in virtual datetime

A virtual datetime (named `vtime` in the language) is a composite type with the components year, month, day, hour, minute and second, and every component can be left unspecified (`vtime { month: 10, day: 26 }` is a datetime with the year and the time of day unspecified). The ranges of the components are only the structural limits (month 1..12, day 1..31, hour 0..23, minute and second 0..59, year 1..9999); whether it is valid as a calendar date (such as February 30) and its meaning are decided by the app's rules.

- An unspecified component cannot be read without checking. If there is a path that may access an unspecified one, the checker makes it an error (after checking with `if datetime.month is null { return … }`, it can be read).
- Conversion to a real datetime (`time`) cannot be written yet. The policy is to write conversions as one-way conversion declarations, to be designed.

## Values received from outside, and conversion

A value received from outside is converted to an internal logical type at the boundary and validated (a conversion on the reading side; execution is only as many times as defined). This conversion is held by the generator, so no keyword such as `raw` is written (it would have no effect on the check or on the generated code). Conversion between logical types is written `convert name(argument: type) -> type = expr`. The range of the result is shown statically by the checker from the range of the argument.

## Converting a virtual datetime to a real datetime

The conversion is written as a geas expression (calendar calculation can be written with only integer addition, subtraction, multiplication, `/` and `%`). The rule for supplying unspecified components is written in the conversion's argument as `fill { hour: 0, minute: 0 }`. A component that has a supplying rule is treated as "specified", and needs no check before reading. If there is a path that reads a component without a rule, it is a violation (the checker stops it). The default time zone is UTC (until the app decides otherwise).

## Time zones

**The idea: values are stored and calculated, all of them, as a single point in UTC (a real datetime). Time zones are handled at the boundaries where something is shown to or received from a person.** The tables (tz data) of named time zones with daylight saving time (`America/New_York`) are not held inside the language. Display and input are the job of boundary conversion (the screen side), and no tz-data library is added to the generated Rust.

The exception is when the local calendar date enters a business rule (business days, the day's closing, daily totals). The same instant has a different date depending on the region, so changing only the display is not enough. In this case, a **fixed offset (in seconds)** from UTC is passed as an argument of the rule (conversion). For a region with daylight saving time, the offset at that moment is produced at the boundary (the screen side) and passed.

- An app can declare one default offset as `timezone +09:00` (UTC if not written; up to ±14:00; cannot be written in a module).
- The declared value can be used inside expressions as the built-in `tz_offset` (an integer of seconds). It is replaced by the value itself, so the checker knows the range exactly.
- A conversion's arguments can have default values: `convert real_datetime(v: vtime fill { … }, offset: Offset = tz_offset) -> time = …`. If the argument is omitted when calling, the default offset is used; if passed, that value overrides it. The intended uses are apps that handle several time zones and apps where the user overrides with their own.
- A default value must fit the argument's type and range. Every argument after one with a default value must also have a default value.
- Negative numbers can be written at the ends of a range (`ltype Offset = int -50400..50400`).

Named time zones, and how to resolve times that do not exist or occur twice because of daylight saving, are placed outside the language (at the boundary), so the language does not decide them. `time(integer)` makes a real datetime from the number of seconds since 1970-01-01 UTC.
