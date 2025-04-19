use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    let poll_interval = Duration::from_secs(1);

    loop {
        let temp = get_max_temp();

        let speed = match temp {
            t if t <= temp_curve[0] => idle_speed,
            t if t >= temp_curve[temp_curve.len() - 1] => speed_curve[speed_curve.len() - 1],
            _ => {
                let index = temp_curve.iter().position(|&t| t > temp).unwrap();
                let t0 = temp_curve[index - 1];
                let t1 = temp_curve[index];
                let s0 = speed_curve[index - 1];
                let s1 = speed_curve[index];
                (s0 as usize + ((s1 - s0) as usize * (temp - t0) as usize / (t1 - t0) as usize)) as u8
            }
        };

        let fan1_speed = ((FAN1_MAX as u16 * speed as u16) / 100) as u8;
        let fan2_speed = ((FAN2_MAX as u16 * speed as u16) / 100) as u8;

        if previous_speed != (fan1_speed, fan2_speed) {
            set_fan_speed(fan1_speed, fan2_speed);
            previous_speed = (fan1_speed, fan2_speed);
        }

        sleep(poll_interval).await;
    }
}

while nix::unistd::Uid::effective().is_root() {
    let temp = get_max_temp();

    let speed = match temp {
        t if t <= temp_curve[0] => idle_speed,
        t if t >= temp_curve[temp_curve.len() - 1] => speed_curve[speed_curve.len() - 1],
        _ => {
            let index = temp_curve.iter().position(|&t| t > temp).unwrap();
            let t0 = temp_curve[index - 1];
            let t1 = temp_curve[index];
            let s0 = speed_curve[index - 1];
            let s1 = speed_curve[index];
            (s0 as usize + ((s1 - s0) as usize * (temp - t0) as usize / (t1 - t0) as usize)) as u8
        }
    };

    let fan1_speed = ((FAN1_MAX as u16 * speed as u16) / 100) as u8;
    let fan2_speed = ((FAN2_MAX as u16 * speed as u16) / 100) as u8;

    if previous_speed != (fan1_speed, fan2_speed) {
        set_fan_speed(fan1_speed, fan2_speed);
        previous_speed = (fan1_speed, fan2_speed);
    }

    sleep(poll_interval);
}