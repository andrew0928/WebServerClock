# 0003　校正流程與跨秒樣本的選擇

校正模組負責準備、每輪等待、搜尋邊界及選取結果。呼叫端只接收「準備中」、「校正中（輪次、步長）」及「結果」。每輪結束便可顯示，無須等全部輪次完成。

## C# 介面對照

```csharp
public sealed record CalibrationOptions(int? Count = 4, int? MinStepMs = null); // null 持續執行
public sealed record BoundaryResult(double DiffMs, int StepMs);

public abstract record CalibrationEvent;
public sealed record Preparing() : CalibrationEvent;
public sealed record Calibrating(long Count, int StepMs) : CalibrationEvent;
public sealed record RoundResult(long Count, BoundaryResult? Result) : CalibrationEvent;

public interface ICalibrator
{
    Task<bool> CalibrateAsync(HttpClient client, Uri target, CalibrationOptions options,
                              Action<CalibrationEvent> report);
}
```

Rust 的對應入口為 `calibrate(client, url, options, emit).await`。有限輪數以 `CalibrationOptions::new` 建立，持續模式使用 `CalibrationOptions::continuous`。兩種模式共用一次準備。三種事件都在 `CalibrationEvent` 中定義。回傳的 `bool` 表示準備成功且每輪皆有結果。Rust 的回呼可回傳 I/O 錯誤，讓 CLI 在輸出中斷時停止。[Rust 公開介面](../../src/calibration/session.rs)

## 演算法

1. 進入準備階段，以 1 秒間隔做五次 HEAD。用成功回應的平均 RTT/2 選擇細掃步長。低於 20、50、100 ms 分別選 10、20、25 ms，100 ms 以上只做粗掃。手動指定 1–100 ms 可覆寫步長，指定 100 ms 即只做粗掃。五次全部失敗則停止。
2. 每輪各自抽取 0–1000 ms 的整數延遲，包含兩端。等待後，以 1.100 秒週期發出請求，使相位每次前進 100 ms。每輪最多 20 秒，含隨機等待。
3. 每筆計算 `diff = HTTP Date − 本機請求起訖中點`。一般相鄰樣本的 diff 會減少一個步長；跨越截斷邊界時，diff 會多跳一秒。因此，100 ms 粗掃時預期觀察到約 **+900 ms**，10 ms 細掃時約 **+990 ms**。實作要求相鄰有效樣本的中點間隔和 diff 增量均落在 [0.5, 1.5) 秒，且樣本時差區間相容。失敗取樣或切換掃描區間會中斷配對。
4. **只比較邊界兩側的兩筆 diff，選絕對值較小者，保留正負號。** 絕對值相同時選較晚的一筆。粗掃確認後進入細掃，在細掃確認新邊界時以該對樣本重新選值。到期若只確認粗掃，結果標明 100 ms；整輪都沒有確認跨越或樣本互相矛盾，則回報該輪未取得結果。下一輪從新的隨機延遲重新開始。

## 結果判讀

−996 ms、+4 ms 會選 +4 ms；−994 ms、−4 ms 會選 −4 ms。當網站時鐘接近本機，這個規則能選出接近零的一側。若伺服器實際快 600 ms，細掃時跨秒兩側可能接近 −400 ms 和 +600 ms，規則會選接近 −400 ms 的一側。因此，輸出的步長反映觀察到跨越時的取樣格距，無法保證相對於真實時差的誤差也在該格距內。

## 測試對照

| 層級 | 可觀察的行為 |
|---|---|
| [邊界單元測試](../../src/calibration/tests.rs) | 以已知中點與 Date 模擬跨越，檢查取絕對值、保留符號、略過普通漂移與漏拍、拒絕矛盾樣本，以及 RTT 分級邊界。 |
| [流程單元測試](../../src/calibration/session/tests.rs) | 注入假時鐘、RTT 與亂數。驗證五次準備、每輪抽樣與等待、不同時差的獨立輪次、手動步長、20 秒期限及粗掃退回值。 |
| [CLI 端到端測試](../../tests/calibration_cli.rs) | 對本機 HTTP 伺服器執行 CLI，驗證預設四行、持續模式超過四輪、監控時鐘持續更新，以及 Ctrl+C 還原終端機。 |
