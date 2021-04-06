'use strict'

import React, { useState } from 'react'
import ReactDOM from 'react-dom'
import { Container, Tab } from 'semantic-ui-react'
import Header from './components/Header'
import DataTable from './components/DataTable'
import Footer from './components/Footer'
import 'semantic-ui-css/semantic.min.css'
import './style/app.scss'

const panes = [
  {
    menuItem: 'Records',
    render: () => <Tab.Pane attached={false}><DataTable /></Tab.Pane>
  },
  {
    menuItem: 'Map',
    render: () => <Tab.Pane attached={false}>TODO</Tab.Pane>
  }
]

function App () {
  const [active, setActive] = useState('Records')

  function handleTabChange (e, el) {
    setActive(panes[el.activeIndex].menuItem)
  }

  return (
    <div className='app'>
      <Container>
        <Header title='Lachesis UI' />
        <Tab
          menu={{ secondary: true, pointing: true }}
          panes={panes}
          onTabChange={handleTabChange}
          className={active === 'Records' ? 'nopadding' : ''}
        />
        <Footer version='v0.3.0' />
      </Container>
    </div>
  )
}

ReactDOM.render(<App />, document.querySelector('#root'))
