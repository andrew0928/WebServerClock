# WebServerClock

文章介紹: [網路搶票小幫手 - 遠端網站時鐘, WebServerClock v1.0](https://columns.chicken-house.net/2017/01/05/webserverclock-release/)

## clock-ping 命令列版

Apple Silicon Mac 可用 Homebrew 安裝：

```sh
brew install andrew0928/tools/clock-ping
clock-ping https://example.com -m
```

如需手動安裝，可從 [clock-ping v0.1.0 發行頁](https://github.com/andrew0928/WebServerClock/releases/tag/clock-ping-v0.1.0) 下載 macOS arm64 壓縮檔及 SHA-256 校驗檔。在已安裝 Rust 和 Cargo 的 macOS、Linux 或 Windows 上，也可以從專案根目錄編譯安裝：

```sh
cargo install --path . --locked
clock-ping https://example.com -t
clock-ping https://example.com -m
```

也可直接在專案內執行 `cargo run --locked -- https://example.com -t`。預設每秒量測一次，共四次。加上 `-t` 會持續逐行列印，按 Ctrl+C 結束。單次請求逾時（1 秒）或缺少有效的 `Date` 標頭時，CLI 會印出原因並繼續下一次量測。

執行 `clock-ping --help` 查看參數與範例，`clock-ping --version` 查看版本。離線指南內建於執行檔，可用 `clock-ping --skill` 或 `clock-ping --skill index` 取得主題索引。`clock-ping --skill measure` 說明量測及錯誤處理，`clock-ping --skill author` 提供作者聯絡方式、工具介紹文章與 GitHub 專案網址。

若想知道該測哪個網址，可以直接告訴你的 agent：「請讀 `clock-ping --skill target`，幫我查這個網站實際受理表單的 host、HTTP 時間的來源，以及能從公開資料確認哪些代管資訊。」附上網址及你關心的操作即可。

```text
時間差 +5234.1 ms  遠端時間 15:04:05.012  RTT/2 12.0 ms
```

時間差為正表示遠端時鐘較快，負值表示較慢。CLI 沿用 Windows 版的規則，向網址所屬主機的根路徑發送 `HEAD /`（保留連接埠），以回應的 `Date` 減去本機請求起訖的中點時間。遠端時間是收到標頭當下的估計值，以本機時區顯示。

`RTT/2` 是從送出請求到收到標頭所花時間的一半。HTTP `Date` 只有整秒精度，因此即使往返很快，兩台已對時的設備仍可能顯示數百毫秒的時差。如果網址經過 CDN 或反向代理，量到的是產生該標頭的設備時鐘。

### 監看模式

`-m` 或 `--monitor` 開啟固定畫面，每秒取樣一次。時鐘每 10 ms 以「目前本機時間 ＋ 最近 5 秒有效樣本的平均時差」更新。按 Ctrl+C 回到原本的終端機畫面。

```text
遠端時鐘          15:09:16.143

最新時差          -892.2 ms
平均時差（5 秒）  -889.2 ms
RTT/2             8.6 ms

網址  https://example.com/
標題  Example Domain

最後更新  15:09:16.900（本機）
最近 5 秒  最小 -892.2 ms   最大 -888.0 ms   有效樣本 5 筆

Ctrl+C 結束
```

青色欄名與粗體數值標出最新時差、5 秒平均時差及 `RTT/2`。最後更新時間、範圍與樣本數以低亮度顯示。最新時差與 `RTT/2` 取自同一筆成功樣本。最近 5 秒沒有有效樣本時，時鐘顯示 `--:--:--.---`，上次成功的數值轉為低亮度，直到下次成功。

監看畫面顯示輸入的完整網址，啟動時會對該頁面另送一次 `GET` 讀取 HTML 標題。取得標題時才顯示該欄。每秒的時差量測仍使用 `HEAD /`，只計入該次探測的往返時間。

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
