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


        
        private void button1_Click(object sender, EventArgs e)
        {
            HttpClient client = new HttpClient();
            client.BaseAddress = new Uri(this.textWebSiteURL.Text);

            HttpRequestMessage hrm = new HttpRequestMessage(HttpMethod.Head, "/");

            DateTime start = DateTime.Now;
            HttpResponseMessage rsp = client.SendAsync(hrm, HttpCompletionOption.ResponseHeadersRead).Result;
            TimeSpan deviation = DateTime.Now - start;

            this.Offset = DateTime.Parse(rsp.Headers.GetValues("Date").First()) - DateTime.Now;
            this.labelOffset.Text = string.Format(
                @"時間差: {0} msec, 最大誤差值: {1} msec", 
                this.Offset.TotalMilliseconds,
                deviation.TotalMilliseconds);

        }
    }
}
