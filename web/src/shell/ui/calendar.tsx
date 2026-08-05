import { enUS, zhCN } from "date-fns/locale"
import {
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
} from "lucide-react"
import * as React from "react"
import type { DayButton} from "react-day-picker";
import { DayPicker, getDefaultClassNames } from "react-day-picker"
import { useTranslation } from "react-i18next"

import { cn } from "@/shell/lib/cn"
import { Button, buttonVariants } from "@/shell/ui/button"

function Calendar({
  className,
  classNames,
  showOutsideDays = true,
  captionLayout = "label",
  buttonVariant = "ghost",
  formatters,
  labels,
  locale,
  components,
  ...props
}: React.ComponentProps<typeof DayPicker> & {
  buttonVariant?: React.ComponentProps<typeof Button>["variant"]
}) {
  const { t, i18n } = useTranslation("common")
  const defaultClassNames = getDefaultClassNames()
  const language = i18n.resolvedLanguage ?? i18n.language
  const intlLocale = language?.toLowerCase().startsWith("zh")
    ? "zh-CN"
    : "en-US"
  const resolvedLocale =
    locale ?? (intlLocale === "zh-CN" ? zhCN : enUS)
  const formatDate = (
    date: Date,
    options: Intl.DateTimeFormatOptions
  ) => new Intl.DateTimeFormat(intlLocale, options).format(date)

  return (
    <DayPicker
      showOutsideDays={showOutsideDays}
      className={cn(
        "bg-background group/calendar p-3 [--cell-size:2rem] [[data-slot=card-content]_&]:bg-transparent [[data-slot=popover-content]_&]:bg-transparent",
        String.raw`rtl:**:[.rdp-button\_next>svg]:rotate-180`,
        String.raw`rtl:**:[.rdp-button\_previous>svg]:rotate-180`,
        className
      )}
      captionLayout={captionLayout}
      locale={resolvedLocale}
      formatters={{
        formatCaption: (date) =>
          formatDate(date, { month: "long", year: "numeric" }),
        formatDay: (date) => formatDate(date, { day: "numeric" }),
        formatMonthDropdown: (date) =>
          formatDate(date, { month: "short" }),
        formatWeekdayName: (date) =>
          formatDate(date, { weekday: "short" }),
        formatYearDropdown: (date) =>
          formatDate(date, { year: "numeric" }),
        ...formatters,
      }}
      labels={{
        labelNav: () => t("date_time_picker.calendar_navigation"),
        labelGrid: (date) =>
          formatDate(date, { month: "long", year: "numeric" }),
        labelGridcell: (date) =>
          formatDate(date, {
            day: "numeric",
            month: "long",
            year: "numeric",
          }),
        labelMonthDropdown: () => t("date_time_picker.choose_month"),
        labelYearDropdown: () => t("date_time_picker.choose_year"),
        labelNext: () => t("date_time_picker.next_month"),
        labelPrevious: () => t("date_time_picker.previous_month"),
        labelDayButton: (date, modifiers) => {
          const state = [
            modifiers.today ? t("date_time_picker.today") : "",
            modifiers.selected ? t("date_time_picker.selected") : "",
          ].filter(Boolean)
          const formatted = formatDate(date, {
            day: "numeric",
            month: "long",
            year: "numeric",
            weekday: "long",
          })
          return state.length > 0
            ? `${formatted}, ${state.join(", ")}`
            : formatted
        },
        labelWeekday: (date) =>
          formatDate(date, { weekday: "long" }),
        labelWeekNumber: (weekNumber) =>
          t("date_time_picker.week_number", { value: weekNumber }),
        labelWeekNumberHeader: () =>
          t("date_time_picker.week_number_header"),
        ...labels,
      }}
      classNames={{
        root: cn("w-fit", defaultClassNames.root),
        months: cn(
          "relative flex flex-col gap-4 md:flex-row",
          defaultClassNames.months
        ),
        month: cn("flex w-full flex-col gap-4", defaultClassNames.month),
        nav: cn(
          "absolute inset-x-0 top-0 flex w-full items-center justify-between gap-1",
          defaultClassNames.nav
        ),
        button_previous: cn(
          buttonVariants({ variant: buttonVariant }),
          "h-[--cell-size] w-[--cell-size] select-none p-0 aria-disabled:opacity-50",
          defaultClassNames.button_previous
        ),
        button_next: cn(
          buttonVariants({ variant: buttonVariant }),
          "h-[--cell-size] w-[--cell-size] select-none p-0 aria-disabled:opacity-50",
          defaultClassNames.button_next
        ),
        month_caption: cn(
          "flex h-[--cell-size] w-full items-center justify-center px-[--cell-size]",
          defaultClassNames.month_caption
        ),
        dropdowns: cn(
          "flex h-[--cell-size] w-full items-center justify-center gap-1.5 text-sm font-medium",
          defaultClassNames.dropdowns
        ),
        dropdown_root: cn(
          "border-input shadow-xs relative rounded-md border",
          defaultClassNames.dropdown_root
        ),
        dropdown: cn(
          "bg-popover absolute inset-0 opacity-0",
          defaultClassNames.dropdown
        ),
        caption_label: cn(
          "select-none font-medium",
          captionLayout === "label"
            ? "text-sm"
            : "[&>svg]:text-muted-foreground flex h-8 items-center gap-1 rounded-md pl-2 pr-1 text-sm [&>svg]:size-3.5",
          defaultClassNames.caption_label
        ),
        // Phase 6 M0.3: react-day-picker v9 removed the `table` classNames
        // key — the table element styling is now handled internally.
        weekdays: cn("flex", defaultClassNames.weekdays),
        weekday: cn(
          "text-muted-foreground flex-1 select-none rounded-md text-[0.8rem] font-normal",
          defaultClassNames.weekday
        ),
        week: cn("mt-2 flex w-full", defaultClassNames.week),
        week_number_header: cn(
          "w-[--cell-size] select-none",
          defaultClassNames.week_number_header
        ),
        week_number: cn(
          "text-muted-foreground select-none text-[0.8rem]",
          defaultClassNames.week_number
        ),
        day: cn(
          "group/day relative aspect-square h-full w-full select-none p-0 text-center [&:first-child[data-selected=true]_button]:rounded-l-md [&:last-child[data-selected=true]_button]:rounded-r-md",
          defaultClassNames.day
        ),
        range_start: cn(
          "bg-accent rounded-l-md",
          defaultClassNames.range_start
        ),
        range_middle: cn("rounded-none", defaultClassNames.range_middle),
        range_end: cn("bg-accent rounded-r-md", defaultClassNames.range_end),
        today: cn(
          "bg-transparent text-tx-0",
          defaultClassNames.today
        ),
        outside: cn(
          "text-muted-foreground aria-selected:text-muted-foreground",
          defaultClassNames.outside
        ),
        disabled: cn(
          "text-muted-foreground opacity-50",
          defaultClassNames.disabled
        ),
        hidden: cn("invisible", defaultClassNames.hidden),
        ...classNames,
      }}
      components={{
        Root: ({ className, rootRef, ...props }) => {
          return (
            <div
              data-slot="calendar"
              ref={rootRef}
              className={cn(className)}
              {...props}
            />
          )
        },
        Chevron: ({ className, orientation, ...props }) => {
          if (orientation === "left") {
            return (
              <ChevronLeftIcon className={cn("size-4", className)} {...props} />
            )
          }

          if (orientation === "right") {
            return (
              <ChevronRightIcon
                className={cn("size-4", className)}
                {...props}
              />
            )
          }

          return (
            <ChevronDownIcon className={cn("size-4", className)} {...props} />
          )
        },
        DayButton: CalendarDayButton,
        WeekNumber: ({ children, ...props }) => {
          return (
            <td {...props}>
              <div className="flex size-[--cell-size] items-center justify-center text-center">
                {children}
              </div>
            </td>
          )
        },
        ...components,
      }}
      {...props}
    />
  )
}

function CalendarDayButton({
  className,
  day,
  modifiers,
  ...props
}: React.ComponentProps<typeof DayButton>) {
  const defaultClassNames = getDefaultClassNames()

  const ref = React.useRef<HTMLButtonElement>(null)
  React.useEffect(() => {
    if (modifiers.focused) ref.current?.focus()
  }, [modifiers.focused])

  return (
    <Button
      ref={ref}
      variant="ghost"
      size="icon"
      data-day={day.date.toLocaleDateString()}
      data-selected-single={
        modifiers.selected &&
        !modifiers.range_start &&
        !modifiers.range_end &&
        !modifiers.range_middle
      }
      data-range-start={modifiers.range_start}
      data-range-end={modifiers.range_end}
      data-range-middle={modifiers.range_middle}
      className={cn(
        "flex aspect-square h-auto w-full min-w-[--cell-size] flex-col gap-1 rounded-full font-normal leading-none focus-visible:bg-indigo-dim data-[selected-single=true]:bg-primary data-[selected-single=true]:text-primary-foreground data-[selected-single=true]:hover:bg-primary data-[range-middle=true]:rounded-none data-[range-middle=true]:bg-accent data-[range-middle=true]:text-accent-foreground data-[range-start=true]:rounded-md data-[range-start=true]:bg-primary data-[range-start=true]:text-primary-foreground data-[range-end=true]:rounded-md data-[range-end=true]:bg-primary data-[range-end=true]:text-primary-foreground group-data-[focused=true]/day:relative group-data-[focused=true]/day:z-10 group-data-[focused=true]/day:bg-indigo-dim group-data-[focused=true]/day:data-[selected-single=true]:bg-primary [&>span]:text-xs [&>span]:opacity-70",
        defaultClassNames.day,
        className
      )}
      {...props}
    />
  )
}

export { Calendar, CalendarDayButton }
