/**
 * \file flow-rs/src/node/reorder.rs
 * MegFlow is Licensed under the Apache License, Version 2.0 (the "License")
 *
 * Copyright (c) 2019-2021 Megvii Inc. All rights reserved.
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT ARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 */
// 原版提交 95f870bfefd48fa31f9cf88320de4cc177985c72 的 exec 方法原文。
// 仅外部类型和 I/O 由测试适配；方法体未改写。用于独立对照重写算法。
impl ReferenceReorder {
    async fn exec(&mut self, _: &Context) -> Result<()> {
        if let Ok(msg) = self.inp.recv_any().await {
            let id = msg
                .info()
                .partial_id
                .expect("partial_id required by reorder");
            assert!(id >= self.seq_id);
            if id == self.seq_id {
                self.seq_id += 1;
                self.out.send_any(msg).await.ok();
            } else {
                self.cache.insert(id, msg);
            }

            let mut stop = self.seq_id;
            for &id in self.cache.keys() {
                assert!(id >= self.seq_id);
                if id == self.seq_id {
                    self.seq_id += 1;
                } else {
                    stop = id;
                    break;
                }
            }
            if stop != self.seq_id {
                let rest = if stop > self.seq_id {
                    let rest = self.cache.split_off(&stop);
                    std::mem::replace(&mut self.cache, rest)
                } else {
                    std::mem::take(&mut self.cache)
                };

                for (_, msg) in rest {
                    self.out.send_any(msg).await.ok();
                }
            }
        } else {
            assert!(self.cache.is_empty());
        }
        Ok(())
    }
}
