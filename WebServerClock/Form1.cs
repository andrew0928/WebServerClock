using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Data;
using System.Drawing;
using System.Linq;
using System.Net.Http;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace WebServerClock
{
    public partial class Form1 : Form
    {
        public Form1()
        {
            InitializeComponent();
        }

        private void timer1_Tick(object sender, EventArgs e)
        {
            this.labClock.Text = (DateTime.Now + this.Offset).ToString("HH:mm:ss.f");
        }

        private TimeSpan Offset = TimeSpan.Zero;


        
        private async void button1_Click(object sender, EventArgs e)
        {
            HttpClient client = new HttpClient();
            client.BaseAddress = new Uri(this.textWebSiteURL.Text);

            HttpRequestMessage req = new HttpRequestMessage(HttpMethod.Head, "/");

            DateTime t0 = DateTime.Now;
            HttpResponseMessage rsp = await client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead);
            DateTime t3 = DateTime.Now;
            TimeSpan duration = t3 - t0;

            if (rsp.Headers.Date.HasValue)
            {
                DateTime t1p = rsp.Headers.Date.Value.DateTime; //DateTime.Parse(rsp.Headers.GetValues("Date").First());
                this.Offset = t1p - t0.AddMilliseconds(duration.TotalMilliseconds / 2);

                this.labelOffset.Text = string.Format(
                    @"時間差: {0} msec, 最大誤差值: {1} msec",
                    this.Offset.TotalMilliseconds,
                    duration.TotalMilliseconds / 2);
            }
            else
            {
                this.labelOffset.Text = @"網站沒有傳回 Date header 資料。";
            }

        }
    }
}
