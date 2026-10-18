use maya_domain::csv::{Csv, belopp};

#[test]
fn csv_har_bom_semikolon_crlf_och_citerar_fält_som_behöver_det() {
    let mut csv = Csv::new(&["Konto", "Text"]);
    csv.rad(["1930", "Bank; SEB"]);
    csv.rad(["2440".to_string(), "Säger \"hej\"\nrad två".to_string()]);

    assert_eq!(
        csv.text(),
        "\u{feff}Konto;Text\r\n1930;\"Bank; SEB\"\r\n2440;\"Säger \"\"hej\"\"\nrad två\"\r\n"
    );
}

#[test]
fn belopp_har_decimalkomma_minus_och_inga_tusentalsavgränsare() {
    assert_eq!(belopp(123_450), "1234,50");
    assert_eq!(belopp(-5), "-0,05");
    assert_eq!(belopp(0), "0,00");
}
