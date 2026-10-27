use std::time::Duration;

pub fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];

    if bytes == 0 {
        return "0 Б".to_owned();
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn files(count: u64) -> String {
    plural(count, "файл", "файла", "файлов")
}

pub fn records(count: u64) -> String {
    plural(count, "запись", "записи", "записей")
}

pub fn visits(count: u64) -> String {
    plural(count, "посещение", "посещения", "посещений")
}

fn plural(count: u64, one: &str, few: &str, many: &str) -> String {
    let word = if (11..=14).contains(&(count % 100)) {
        many
    } else {
        match count % 10 {
            1 => one,
            2..=4 => few,
            _ => many,
        }
    };

    format!("{count} {word}")
}

pub fn elapsed(duration: Duration) -> String {
    if duration.as_secs() < 1 {
        format!("{} мс", duration.as_millis())
    } else {
        format!("{:.1} с", duration.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_word_forms() {
        assert_eq!(files(0), "0 файлов");
        assert_eq!(files(1), "1 файл");
        assert_eq!(files(2), "2 файла");
        assert_eq!(files(5), "5 файлов");
        assert_eq!(files(11), "11 файлов");
        assert_eq!(files(21), "21 файл");
        assert_eq!(files(111), "111 файлов");
    }

    #[test]
    fn record_word_forms() {
        assert_eq!(records(0), "0 записей");
        assert_eq!(records(1), "1 запись");
        assert_eq!(records(2), "2 записи");
        assert_eq!(records(5), "5 записей");
        assert_eq!(records(11), "11 записей");
        assert_eq!(records(21), "21 запись");
    }

    #[test]
    fn visit_word_forms() {
        assert_eq!(visits(0), "0 посещений");
        assert_eq!(visits(1), "1 посещение");
        assert_eq!(visits(2), "2 посещения");
        assert_eq!(visits(5), "5 посещений");
        assert_eq!(visits(11), "11 посещений");
        assert_eq!(visits(21), "21 посещение");
    }

    #[test]
    fn size_units() {
        assert_eq!(bytes(0), "0 Б");
        assert_eq!(bytes(512), "512 Б");
        assert_eq!(bytes(1024), "1.0 КБ");
        assert_eq!(bytes(1024 * 1024), "1.0 МБ");
    }
}
