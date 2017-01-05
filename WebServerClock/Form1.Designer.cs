namespace WebServerClock
{
    partial class Form1
    {
        /// <summary>
        /// Required designer variable.
        /// </summary>
        private System.ComponentModel.IContainer components = null;

        /// <summary>
        /// Clean up any resources being used.
        /// </summary>
        /// <param name="disposing">true if managed resources should be disposed; otherwise, false.</param>
        protected override void Dispose(bool disposing)
        {
            if (disposing && (components != null))
            {
                components.Dispose();
            }
            base.Dispose(disposing);
        }

        #region Windows Form Designer generated code

        /// <summary>
        /// Required method for Designer support - do not modify
        /// the contents of this method with the code editor.
        /// </summary>
        private void InitializeComponent()
        {
            this.components = new System.ComponentModel.Container();
            this.labWebSite = new System.Windows.Forms.Label();
            this.timer1 = new System.Windows.Forms.Timer(this.components);
            this.textWebSiteURL = new System.Windows.Forms.TextBox();
            this.button1 = new System.Windows.Forms.Button();
            this.labClock = new System.Windows.Forms.Label();
            this.labelOffset = new System.Windows.Forms.Label();
            this.SuspendLayout();
            // 
            // labWebSite
            // 
            this.labWebSite.AutoSize = true;
            this.labWebSite.Font = new System.Drawing.Font("PMingLiU", 12F, System.Drawing.FontStyle.Regular, System.Drawing.GraphicsUnit.Point, ((byte)(136)));
            this.labWebSite.Location = new System.Drawing.Point(12, 9);
            this.labWebSite.Name = "labWebSite";
            this.labWebSite.Size = new System.Drawing.Size(44, 16);
            this.labWebSite.TabIndex = 0;
            this.labWebSite.Text = "網站:";
            // 
            // timer1
            // 
            this.timer1.Enabled = true;
            this.timer1.Interval = 30;
            this.timer1.Tick += new System.EventHandler(this.timer1_Tick);
            // 
            // textWebSiteURL
            // 
            this.textWebSiteURL.Location = new System.Drawing.Point(62, 3);
            this.textWebSiteURL.Name = "textWebSiteURL";
            this.textWebSiteURL.Size = new System.Drawing.Size(386, 22);
            this.textWebSiteURL.TabIndex = 1;
            // 
            // button1
            // 
            this.button1.Font = new System.Drawing.Font("PMingLiU", 12F, System.Drawing.FontStyle.Regular, System.Drawing.GraphicsUnit.Point, ((byte)(136)));
            this.button1.Location = new System.Drawing.Point(454, 3);
            this.button1.Name = "button1";
            this.button1.Size = new System.Drawing.Size(94, 23);
            this.button1.TabIndex = 2;
            this.button1.Text = "同步";
            this.button1.UseVisualStyleBackColor = true;
            this.button1.Click += new System.EventHandler(this.button1_Click);
            // 
            // labClock
            // 
            this.labClock.AutoSize = true;
            this.labClock.Font = new System.Drawing.Font("Microsoft JhengHei UI", 48F, System.Drawing.FontStyle.Bold, System.Drawing.GraphicsUnit.Point, ((byte)(136)));
            this.labClock.Location = new System.Drawing.Point(116, 39);
            this.labClock.Name = "labClock";
            this.labClock.Size = new System.Drawing.Size(425, 81);
            this.labClock.TabIndex = 3;
            this.labClock.Text = "23:59:59.999";
            // 
            // labelOffset
            // 
            this.labelOffset.AutoSize = true;
            this.labelOffset.Location = new System.Drawing.Point(12, 120);
            this.labelOffset.Name = "labelOffset";
            this.labelOffset.Size = new System.Drawing.Size(243, 12);
            this.labelOffset.TabIndex = 4;
            this.labelOffset.Text = "時間差: +00000 msec, 最大誤差值: +00000 msec";
            // 
            // Form1
            // 
            this.AutoScaleDimensions = new System.Drawing.SizeF(6F, 12F);
            this.AutoScaleMode = System.Windows.Forms.AutoScaleMode.Font;
            this.ClientSize = new System.Drawing.Size(553, 141);
            this.Controls.Add(this.labelOffset);
            this.Controls.Add(this.labClock);
            this.Controls.Add(this.button1);
            this.Controls.Add(this.textWebSiteURL);
            this.Controls.Add(this.labWebSite);
            this.Name = "Form1";
            this.Text = "網站同步時鐘 v1.0, By Andrew Wu";
            this.ResumeLayout(false);
            this.PerformLayout();

        }

        #endregion

        private System.Windows.Forms.Label labWebSite;
        private System.Windows.Forms.Timer timer1;
        private System.Windows.Forms.TextBox textWebSiteURL;
        private System.Windows.Forms.Button button1;
        private System.Windows.Forms.Label labClock;
        private System.Windows.Forms.Label labelOffset;
    }
}

