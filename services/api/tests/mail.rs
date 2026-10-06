use lettre::Message;
use scenecask_api::mail::{MailTransport, SmtpMailer};

#[tokio::test]
async fn delivers_to_local_smtp_capture() {
    let host = std::env::var("SMTP_HOST").expect("SMTP_HOST required for local capture test");
    let port = std::env::var("SMTP_PORT")
        .expect("SMTP_PORT required")
        .parse()
        .unwrap();
    let message = Message::builder()
        .from("SceneCask <hello@scenecask.test>".parse().unwrap())
        .to("Foundation <foundation@scenecask.test>".parse().unwrap())
        .subject("SceneCask foundation smoke")
        .body("Local capture verification".to_owned())
        .unwrap();
    SmtpMailer::local_capture(&host, port)
        .send(message)
        .await
        .unwrap();
}

#[tokio::test]
async fn delivery_failure_is_redacted() {
    let message = Message::builder()
        .from("private@scenecask.test".parse().unwrap())
        .to("private-recipient@scenecask.test".parse().unwrap())
        .body("private message".to_owned())
        .unwrap();
    assert_eq!(
        SmtpMailer::local_capture("127.0.0.1", 1)
            .send(message)
            .await,
        Err("email delivery unavailable")
    );
}
