/**
 * \file flow-rs/src/channel/receiver.rs
 * MegFlow is Licensed under the Apache License, Version 2.0 (the "License")
 *
 * Copyright (c) 2019-2021 Megvii Inc. All rights reserved.
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT ARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 */
// 原版提交 95f870bfefd48fa31f9cf88320de4cc177985c72；方法原文，仅外围 I/O 适配。
impl ReferenceReceiver {
    pub async fn batch_recv_any(
        &self,
        n: usize,
        dur: Duration,
    ) -> Result<Vec<SealedEnvelope>, BatchRecvError<SealedEnvelope>> {
        if n == 0 {
            return Ok(Vec::with_capacity(0));
        }
        let mut wait_recv = FuturesUnordered::new();
        wait_recv.push(self.recv_any());
        let timeout = crate::rt::time::sleep(dur).fuse();
        let mut weight = 0;
        let mut batch = Vec::with_capacity(n);
        pin_mut!(timeout);
        loop {
            select! {
                _ = timeout => {
                    return Ok(batch);
                }
                msg = wait_recv.select_next_some() => {
                    match msg {
                        Ok(msg) => {
                            weight += msg.info().weight.unwrap_or(1);
                            batch.push(msg);
                            if weight >= n {
                                return Ok(batch);
                            } else {
                                wait_recv.push(self.recv_any());
                            }
                        },
                        Err(_) => {
                            return Err(BatchRecvError::Closed(batch));
                        }
                    }

                }
            }
        }
    }
}
