# WebServerClock

文章介紹: [網路搶票小幫手 - 遠端網站時鐘, WebServerClock v1.0](https://columns.chicken-house.net/2017/01/05/webserverclock-release/)

## clock-ping 命令列版

### 安裝 0.2.0

在專案根目錄執行以下指令，需要 Rust 與 Cargo。

```sh
cargo install --path . --locked
clock-ping --version
```

Apple Silicon Mac 也可透過 `brew install andrew0928/tools/clock-ping` 安裝套件庫提供的版本。歷次發行的執行檔可在專案發行頁查閱。[GitHub Releases](https://github.com/andrew0928/WebServerClock/releases)

### 逐行量測

```sh
clock-ping https://example.com
clock-ping https://example.com -t
clock-ping https://example.com --count 2 --min-step-ms 25
```

0.2.0 預設使用跳秒校正。啟動時以 1 秒間隔做五次準備量測，之後每完成一輪校正輸出一行。預設四輪，可用 `--count` 指定輪數，或用 `-t` 持續到 Ctrl+C。每輪最多 20 秒。

```text
結果 count = 1（10 ms）：+4.0 ms
```

結果是網站時間減本機時間，括號為實際確認跳秒的步長。時差取自指定網址最終回應的 HTTP `Date`。以絕對值選取邊界樣本的限制見量測指南。[量測指南](guides/measure.md) · [校正演算法與測試](docs/decision/0003-calibration-session.md)

執行 `clock-ping --help` 查看參數與範例，`clock-ping --version` 查看版本。離線指南內建於執行檔，可用 `clock-ping --skill` 或 `clock-ping --skill index` 取得主題索引。`clock-ping --skill measure` 說明量測及錯誤處理，`clock-ping --skill author` 提供作者聯絡方式、工具介紹文章與 GitHub 專案網址。

若想知道該測哪個網址，可以直接告訴你的 agent：「請讀 `clock-ping --skill target`，幫我查這個網站實際受理表單的 host、HTTP 時間的來源，以及能從公開資料確認哪些代管資訊。」附上網址及你關心的操作即可。

### 監控模式

`clock-ping https://example.com --monitor`（或 `-m`）在第一筆校正成功後開啟完整畫面。遠端時鐘每 10 ms 以「本機目前時間 ＋ 最近一次成功的校正時差」更新。後續校正在背景持續執行，成功時立即套用新時差；某輪失敗時，時鐘繼續採用上一筆成功值。

畫面顯示最新時差、對應的輪次與步長、目前進度、網址及最後更新時間。取得網頁標題時也會顯示。按 Ctrl+C 結束並回到原本的終端機畫面。

# Release Note
能透過 Http Response Header (Date) 偵測網站的時鐘, 顯示該網站目前的時間的小工具。
通常在搶購演唱會門票，購物網站限時搶購，或是連續假期訂購車票時，都需要在開放購買的那一秒鐘按下按鈕，此時搭配這個工具就能精準地掌握 server 的時間。  

# System Requirement（Windows 視窗版）
執行環境:
- Microsoft .NET Framework 4.6.1

開發環境:
- Microsoft .NET Framework 4.6.1
- Microsoft Visual Studio 2015

# Changes Log

## ver [1.0](https://github.com/andrew0928/WebServerClock/tree/v1.0) (2017/01/04) first release  
已知問題:
- URL 未做任何防呆及錯誤處理
- 尚未提供 binary release / installation package


# 授權方式採用 [MIT](LICENSE) 授權
